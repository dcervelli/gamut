# The Homebrew formula: what `brew install` builds gamut on a Mac from.
#
# It names the same released tarball as the PKGBUILD, so the two carry the
# same checksum; `bin/release` sets the version in both and
# `bin/pkgbuild-sha` the checksum. A tap is a repository of its own, so this
# file is copied into it after each release rather than read from here.
class Gamut < Formula
  desc "GPU image viewer with color management for photographs, measurements and HDR"
  homepage "https://github.com/dcervelli/gamut"
  url "https://github.com/dcervelli/gamut/archive/refs/tags/v0.7.0.tar.gz"
  sha256 "ebf8187e86868a9d4e6e4cb35ef1304296aab5f0032836e509b327a74640c904"
  license any_of: ["MIT", "Apache-2.0"]

  depends_on "pkgconf" => :build
  depends_on "rust" => :build
  # libheif reads HEIC and AVIF, with whichever codecs Homebrew builds it
  # with; libraw develops camera raw files, 0.21 or newer, which `build.rs`
  # checks. little-cms2 is linked directly as well, because libraw's
  # pkg-config file names it among the libraries to link.
  depends_on "libheif"
  depends_on "libraw"
  depends_on "little-cms2"
  # Linux is the PKGBUILD's, where the window, the file dialog and the
  # clipboard need the desktop's own libraries rather than Homebrew's.
  depends_on :macos

  def install
    system "cargo", "install", *std_cargo_args

    # The manual page and the example configuration come out of the binary
    # just built, so they describe this version of the program and no other.
    (man1/"gamut.1").write Utils.safe_popen_read(bin/"gamut", "--print-man")
    (doc/"config.example").write Utils.safe_popen_read(bin/"gamut", "--print-config")
    doc.install "user-docs/KEYS.md", "user-docs/FORMATS.md", "user-docs/SETTINGS.md"

    bash_completion.install "packaging/completions/gamut.bash" => "gamut"
    zsh_completion.install "packaging/completions/_gamut"
    fish_completion.install "packaging/completions/gamut.fish"
  end

  test do
    assert_equal "gamut #{version}", shell_output("#{bin}/gamut --version").strip
    # A file that is not what its name says is refused on the command line,
    # before any window: the decoders are linked and reading.
    (testpath/"broken.png").write "not a picture"
    assert_match "reading the header", shell_output("#{bin}/gamut #{testpath}/broken.png 2>&1", 1)
  end
end
