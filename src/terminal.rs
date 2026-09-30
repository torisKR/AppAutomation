//! Small, dependency-free Unix terminal adapter. Keep ISIG enabled so Ctrl+C
//! continues to reach AppForge and any active CLI in the terminal process group.
use std::io;
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::process::{Command, Stdio};

pub struct Session {
    #[cfg(unix)]
    saved: String,
}

impl Session {
    pub fn enter() -> io::Result<Self> {
        #[cfg(unix)]
        {
            let output = stty(&["-g"])?;
            let saved = String::from_utf8_lossy(&output).trim().to_owned();
            if saved.is_empty() {
                return Err(io::Error::other("stty returned no terminal state"));
            }
            // Construct the guard before changing anything, including fallible writes.
            let session = Self { saved };
            stty(&["-icanon", "-echo", "-ixon", "isig", "min", "1", "time", "0"])?;
            print!("\x1b[?1049h\x1b[?1000h\x1b[?1006h\x1b[?2004h");
            io::stdout().flush()?;
            Ok(session)
        }
        #[cfg(not(unix))]
        Ok(Self {})
    }

    pub fn mouse_enabled(&self) -> bool {
        cfg!(unix)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = io::stdout().write_all(b"\x1b[?2004l\x1b[?1006l\x1b[?1000l\x1b[?1049l");
            let _ = io::stdout().flush();
            let _ = stty(&[&self.saved]);
        }
    }
}

#[cfg(unix)]
fn stty(args: &[&str]) -> io::Result<Vec<u8>> {
    let output = Command::new("stty")
        .args(args)
        .stdin(Stdio::inherit())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "stty: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

pub fn size() -> (usize, usize) {
    #[cfg(unix)]
    if let Ok(output) = stty(&["size"]) {
        let text = String::from_utf8_lossy(&output);
        let values = text
            .split_whitespace()
            .filter_map(|v| v.parse::<usize>().ok())
            .collect::<Vec<_>>();
        if let [rows, cols] = values.as_slice() {
            if *rows > 0 && *cols > 0 {
                return (*cols, *rows);
            }
        }
    }
    let dimension = |name, default| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(default)
    };
    (dimension("COLUMNS", 120), dimension("LINES", 24))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Command(String),
    Click { column: usize, row: usize },
    Manual,
}

#[derive(Default)]
pub struct Input {
    line: Vec<u8>,
    escape: Vec<u8>,
    pasting: bool,
    after_cr: bool,
}

impl Input {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.line).into_owned()
    }

    pub fn clear(&mut self) {
        self.line.clear();
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Action> {
        let mut actions = Vec::new();
        for &byte in bytes {
            if !self.escape.is_empty() {
                // A lone Escape must not swallow the next ordinary keystroke.
                if self.escape == [0x1b] && byte != b'[' && byte != b'O' {
                    self.escape.clear();
                    continue;
                }
                self.escape.push(byte);
                if self.escape.len() > 64 {
                    self.escape.clear();
                } else if self.escape.len() > 2 && (0x40..=0x7e).contains(&byte) {
                    match self.escape.as_slice() {
                        b"\x1b[200~" => self.pasting = true,
                        b"\x1b[201~" => self.pasting = false,
                        sequence if !self.pasting => {
                            if let Some(action) = mouse_press(sequence) {
                                actions.push(action);
                            }
                        }
                        _ => {}
                    }
                    self.escape.clear();
                }
                continue;
            }
            if byte == 0x1b {
                self.escape.push(byte);
                continue;
            }
            if self.pasting {
                // Pasted newlines never submit commands or trigger a pipeline.
                if matches!(byte, b'\r' | b'\n' | b'\t') {
                    self.append(b' ');
                } else if byte >= 0x20 && byte != 0x7f {
                    self.append(byte);
                }
                continue;
            }
            match byte {
                3 => {
                    self.clear();
                    actions.push(Action::Manual);
                }
                b'\r' | b'\n' => {
                    if byte != b'\n' || !self.after_cr {
                        actions.push(Action::Command(self.text()));
                        self.clear();
                    }
                }
                8 | 127 => while self.line.pop().is_some_and(|b| b & 0xc0 == 0x80) {},
                21 => self.clear(), // Ctrl+U
                b if b >= 0x20 => self.append(b),
                _ => {}
            }
            self.after_cr = byte == b'\r';
        }
        actions
    }

    fn append(&mut self, byte: u8) {
        if self.line.len() < 16 * 1024 {
            self.line.push(byte);
        }
    }
}

fn mouse_press(sequence: &[u8]) -> Option<Action> {
    let payload = sequence.strip_prefix(b"\x1b[<")?.strip_suffix(b"M")?;
    let text = std::str::from_utf8(payload).ok()?;
    let mut fields = text.split(';');
    let button = fields.next()?.parse::<usize>().ok()?;
    let column = fields.next()?.parse::<usize>().ok()?;
    let row = fields.next()?.parse::<usize>().ok()?;
    // Only an unmodified left-button press, never release/drag/scroll.
    if button != 0 || column == 0 || row == 0 || fields.next().is_some() {
        return None;
    }
    Some(Action::Click { column, row })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_mouse_reports_do_not_submit_text() {
        let mut input = Input::default();
        assert!(input.feed(b"hello\x1b[<0;12;").is_empty());
        assert_eq!(
            input.feed(b"1M"),
            vec![Action::Click { column: 12, row: 1 }]
        );
        assert_eq!(input.text(), "hello");
        for sequence in [
            b"\x1b[<0;12;1m".as_slice(),
            b"\x1b[<32;12;1M",
            b"\x1b[<64;12;1M",
            b"\x1b[<2;12;1M",
            b"\x1b[<0;0;1M",
            b"\x1b[<0;12;1;9M",
            b"\x1b[A",
        ] {
            assert!(input.feed(sequence).is_empty());
        }
        assert_eq!(input.text(), "hello");
    }

    #[test]
    fn utf8_backspace_and_crlf_submit_once() {
        let mut input = Input::default();
        input.feed("a한".as_bytes());
        assert!(input.feed(b"\x7f").is_empty());
        assert_eq!(input.feed(b"\r\n"), vec![Action::Command("a".into())]);
        assert!(input.feed(b"\x1b[3~").is_empty());
        assert_eq!(input.feed(b"\x03"), vec![Action::Manual]);
    }

    #[test]
    fn bracketed_paste_cannot_run_commands_or_click_buttons() {
        let mut input = Input::default();
        assert!(input
            .feed(b"\x1b[200~auto\nrun\rquit\x1b[<0;1;1M\x1b[201~")
            .is_empty());
        assert_eq!(input.text(), "auto run quit");
        assert_eq!(
            input.feed(b"\r"),
            vec![Action::Command("auto run quit".into())]
        );
    }
}
