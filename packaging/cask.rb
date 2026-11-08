# The Homebrew cask: what `brew install --cask gamut` installs on a Mac —
# the signed and notarized Gamut.app the release workflow attaches to each
# release, copied into /Applications, where Finder, "Open With" and the
# Dock find it at the same place from one version to the next.
#
# `bin/release` sets the version and `bin/pkgbuild-sha` the checksum, once
# the release's zip exists, and copies this file into the tap,
# dcervelli/homebrew-gamut, as Casks/gamut.rb. Edit it here.
cask "gamut" do
  version "0.14.0"
  sha256 "2686f6e43ab92290cf1d5f3592c55b16efaefb61eed0a1b8e89ab67182fc5be6"

  url "https://github.com/dcervelli/gamut/releases/download/v#{version}/Gamut-#{version}-arm64.zip"
  name "Gamut"
  desc "GPU image viewer with color management for photographs, measurements and HDR"
  homepage "https://github.com/dcervelli/gamut"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: ">= :sequoia"
  depends_on arch: :arm64
  # The formula installs the same program from source, the command on the
  # path and the bundle with it.
  conflicts_with formula: "gamut"

  app "Gamut.app"
  # The command is a script inside the bundle that runs the program there:
  # macOS gives the program its name, icon and identity only when it is
  # started from inside the bundle, which a link to the binary is not.
  binary "#{appdir}/Gamut.app/Contents/Resources/bin/gamut"
  manpage "#{appdir}/Gamut.app/Contents/Resources/man/man1/gamut.1"
  bash_completion "#{appdir}/Gamut.app/Contents/Resources/completions/gamut.bash"
  zsh_completion "#{appdir}/Gamut.app/Contents/Resources/completions/_gamut"
  fish_completion "#{appdir}/Gamut.app/Contents/Resources/completions/gamut.fish"

  zap trash: [
    "~/.config/gamut",
    "~/Library/Application Support/gamut",
    "~/Library/Caches/com.dcervelli.gamut",
  ]
end
