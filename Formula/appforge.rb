class Appforge < Formula
  desc "Subscription-first multi-agent app factory"
  homepage "https://github.com/torisKR/AppAutomation"
  version "0.2.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/torisKR/AppAutomation/releases/download/v0.2.2/appforge_Darwin_arm64.tar.gz"
      sha256 "a4d6e55c8075a4cb764943f393d8989051e67b80333a4db6d071b3c647614cac"
    else
      url "https://github.com/torisKR/AppAutomation/releases/download/v0.2.2/appforge_Darwin_x86_64.tar.gz"
      sha256 "8bac39de594177fe3a844c33e78c7121ade82e014afb4af2a951fab4bc81e7be"
    end
  end

  on_linux do
    depends_on arch: :x86_64
    url "https://github.com/torisKR/AppAutomation/releases/download/v0.2.2/appforge_Linux_x86_64.tar.gz"
    sha256 "441018bedcbbe79cc0ce5e62679f47934277d707bcfb20bcea5949cc280a9628"
  end

  def install
    bin.install "appforge"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/appforge version")
  end
end
