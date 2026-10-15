# Homebrew cask, served straight from this repository as a tap:
#   brew tap deepakvettickal/caturankam https://github.com/deepakvettickal/caturankam
#   brew install --cask caturankam
# Update `version` and `sha256` (from the release's checksums.txt) after each release.
cask "caturankam" do
  version "1.1.0"
  sha256 "4b5b86ba9ad14258f93f504a7ce2b8d8e59c01af7dad8912a38981106a1e66dd"

  url "https://github.com/deepakvettickal/caturankam/releases/download/v#{version}/Caturankam-#{version}.dmg"
  name "Caturankam"
  desc "Super-lightweight native chess client for Lichess"
  homepage "https://github.com/deepakvettickal/caturankam"

  depends_on macos: ">= :big_sur"

  app "Caturankam.app"

  # Releases aren't notarized by Apple, so macOS would block the first launch.
  # Tapping this repository is the trust decision: clear the download flag.
  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-dr", "com.apple.quarantine", "#{appdir}/Caturankam.app"]
  end

  zap trash: [
    "~/Library/Application Support/caturankam",
    "~/Library/Caches/caturankam",
  ]
end
