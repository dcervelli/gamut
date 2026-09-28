# The Homebrew formula: what `brew install` builds gamut on a Mac from.
#
# It names the same released tarball as the PKGBUILD, so the two carry the
# same checksum; `bin/release` sets the version in both and
# `bin/pkgbuild-sha` the checksum, and then copies this file into the tap,
# dcervelli/homebrew-gamut, which is what Homebrew reads. Edit it here.
class Gamut < Formula
  desc "GPU image viewer with color management for photographs, measurements and HDR"
  homepage "https://github.com/dcervelli/gamut"
  url "https://github.com/dcervelli/gamut/archive/refs/tags/v0.8.1.tar.gz"
  sha256 "5c9891b3e479dc652d3861678e9572d1dd8dcd6556f5c86acf3f86aee29b1757"
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
    system "cargo", "install", *std_cargo_args(root: buildpath/"built")

    # The binary lives in the application bundle, which is what Finder, the
    # Dock and "Open With" know the program by, and the command on the path
    # runs it there: macOS gives a program its bundle's name, icon and
    # identity only when it is started from inside the bundle, which a
    # symlink does not count as.
    system "bin/bundle", buildpath/"built/bin/gamut", prefix
    app = prefix/"Gamut.app/Contents/MacOS/gamut"
    bin.write_exec_script opt_prefix/"Gamut.app/Contents/MacOS/gamut"

    # The manual page and the example configuration come out of the binary
    # just built, so they describe this version of the program and no other.
    (man1/"gamut.1").write Utils.safe_popen_read(app, "--print-man")
    (doc/"config.example").write Utils.safe_popen_read(app, "--print-config")
    doc.install "user-docs/KEYS.md", "user-docs/FORMATS.md", "user-docs/SETTINGS.md"

    bash_completion.install "packaging/completions/gamut.bash" => "gamut"
    zsh_completion.install "packaging/completions/_gamut"
    fish_completion.install "packaging/completions/gamut.fish"
  end

  # Launch Services knows a bundle by the path it was registered at, and an
  # upgrade moves the bundle to a new keg and deletes the old one, so the
  # new bundle is registered here — otherwise "Open With" keeps naming the
  # deleted one, and choosing it does nothing.
  def post_install
    system "/System/Library/Frameworks/CoreServices.framework/Frameworks/" \
           "LaunchServices.framework/Support/lsregister", "-f", prefix/"Gamut.app"
  end

  def caveats
    <<~EOS
      Gamut.app is in #{opt_prefix}. To have it in /Applications:
        ln -sf #{opt_prefix}/Gamut.app /Applications/Gamut.app
    EOS
  end

  test do
    assert_equal "gamut #{version}", shell_output("#{bin}/gamut --version").strip
    # A file that is not what its name says is refused on the command line,
    # before any window: the decoders are linked and reading.
    (testpath/"broken.png").write "not a picture"
    assert_match "reading the header", shell_output("#{bin}/gamut #{testpath}/broken.png 2>&1", 1)
  end
end
