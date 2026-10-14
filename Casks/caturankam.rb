# Homebrew cask, served straight from this repository as a tap:
#   brew tap deepakvettickal/caturankam https://github.com/deepakvettickal/caturankam
#   brew install --cask caturankam
# Update `version` and `sha256` (from the release's checksums.txt) after each release.
cask "caturankam" do
  version "1.0.0"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/deepakvettickal/caturankam/releases/download/v#{version}/Caturankam-#{version}.dmg"
  name "Caturankam"
  desc "Super-lightweight native chess client for Lichess"
  homepage "https://github.com/deepakvettickal/caturankam"

  depends_on macos: ">= :big_sur"

  app "Caturankam.app"

  zap trash: [
    "~/Library/Application Support/caturankam",
    "~/Library/Caches/caturankam",
  ]
end
