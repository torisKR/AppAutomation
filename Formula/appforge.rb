class Appforge < Formula
  desc "Subscription-first multi-agent app factory"
  homepage "https://github.com/torisKR/AppAutomation"
  version "0.3.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/torisKR/AppAutomation/releases/download/v0.3.0/appforge_Darwin_arm64.tar.gz"
      sha256 "9055e9b2ee6249a7e9d0b2cd142b8bcbfa4dacd14fdc72181f5e2a059f114317"
    else
      url "https://github.com/torisKR/AppAutomation/releases/download/v0.3.0/appforge_Darwin_x86_64.tar.gz"
      sha256 "d8b48d7f1ff41c3ba2143f542172454224fd0473efb7b4a9eb21c193639adab3"
    end
  end

  on_linux do
    depends_on arch: :x86_64
    url "https://github.com/torisKR/AppAutomation/releases/download/v0.3.0/appforge_Linux_x86_64.tar.gz"
    sha256 "e4da9d9fff1f84a23b1b827f89efd61249a4d896c4562dcb5407653cc9ad0f6e"
  end

  def install
    bin.install "appforge"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/appforge version")
  end
end
