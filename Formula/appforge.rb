class Appforge < Formula
  desc "Subscription-first multi-agent app factory"
  homepage "https://github.com/torisKR/AppAutomation"
  version "0.1.0"
  license "MIT"
  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/torisKR/AppAutomation/releases/download/v0.1.0/appforge_Darwin_arm64.tar.gz"
      sha256 "1e2308d9b58ede01f2088b1f197c86ec52f91ecc938d2b5734de6a4f0c523810"
    else
      url "https://github.com/torisKR/AppAutomation/releases/download/v0.1.0/appforge_Darwin_x86_64.tar.gz"
      sha256 "f6da845d0d79b9a5b4b7bf9b7b68239e876c95e6409b10fabe88d856e073901b"
    end
  end

  on_linux do
    depends_on arch: :x86_64
    url "https://github.com/torisKR/AppAutomation/releases/download/v0.1.0/appforge_Linux_x86_64.tar.gz"
    sha256 "7f87bd153f1be0ac400e0db1bd2741f35903c57afcabb88db9938f017fb7e7e0"
  end

  def install
    bin.install "appforge"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/appforge version")
  end
end
