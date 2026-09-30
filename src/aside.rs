use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

pub fn available() -> bool {
    which("aside")
}

fn which(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path)
        .map(|p| p.join(name))
        .any(|p| p.is_file())
}

pub fn run_research(project: &Path, stage: &str, product_brief: &str) -> io::Result<String> {
    if !available() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Aside CLI is not installed",
        ));
    }

    let task = match stage {
        "plan" => format!(
            "Research the web for current mobile app/game patterns relevant to this product brief. Focus on market references, store constraints, comparable UX patterns, and implementation risks. Do not purchase, publish, or change account settings. Return concise actionable notes for a coding agent. Product brief: {product_brief}"
        ),
        "design" => format!(
            "Research current mobile game UI/UX references for this brief. Focus on interaction patterns, onboarding, retention loops, accessibility, screen hierarchy, art direction references, and App Store / Play screenshot conventions. Do not copy protected artwork. Return design notes only. Product brief: {product_brief}"
        ),
        "build" => format!(
            "Research current official documentation or reliable implementation references needed to build this mobile app/game. Focus on Expo/React Native where suitable, platform permissions, build constraints, and SDK changes. Return engineering notes and source names; do not change remote accounts. Product brief: {product_brief}"
        ),
        "qa" => format!(
            "Research current mobile QA, privacy, permissions, and store policy checks that may apply to this app/game. Return a concise checklist and risks. Do not submit anything. Product brief: {product_brief}"
        ),
        "store" => format!(
            "Using browser access only for research, inspect current Google Play and Apple App Store listing requirements relevant to this app/game and produce a launch checklist. Do not upload, submit, purchase, or accept legal agreements. Product brief: {product_brief}"
        ),
        "release" => format!(
            "Research the current GitHub Release and distribution checks relevant to this project. If signed-in pages are available, you may inspect them read-only. Do not publish an irreversible store submission. Return a final release verification checklist. Product brief: {product_brief}"
        ),
        _ => format!(
            "Research current web context for this mobile app/game brief and return actionable notes: {product_brief}"
        ),
    };

    let output = Command::new("aside")
        .args(["exec", "--permission", "guard", "--speed", "fast"])
        .arg(task)
        .current_dir(project)
        .output()?;

    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        text.push_str("\n\n[aside diagnostics]\n");
        text.push_str(stderr.trim());
        text.push('\n');
    }

    let dir = project.join("docs/aside");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(format!("{stage}.md")), &text)?;

    if output.status.success() {
        Ok(text)
    } else {
        Err(io::Error::other(format!(
            "Aside exited with {}",
            output.status
        )))
    }
}
