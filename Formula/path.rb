class Path < Formula
  desc "Inspect and manage shell PATH entries safely"
  homepage "https://github.com/compcy/path"
  url "https://github.com/compcy/path/archive/refs/tags/v0.5.0.tar.gz"
  sha256 "f2ca93896604e350208c17447cfeb588de57500a63d63f8bb82db71a9804ed63"
  license "MIT"
  keg_only "wrapper-sourced command to enforce pinned binary and hardening defaults"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
    pkgshare.install "path-wrapper.sh"

    wrapper = pkgshare/"path-wrapper.sh"
    checksum = Utils.safe_popen_read("shasum", "-a", "256", bin/"path").split.first
    wrapper_content = wrapper.read
    defaults_block = <<~SH
      # Homebrew-installed secure defaults (override by setting env vars before sourcing).
      : "${PATH_CLI_BIN:=#{opt_bin}/path}"
      : "${PATH_CLI_ALLOWLIST:=#{opt_bin}}"
      : "${PATH_CLI_SHA256:=#{checksum}}"
      export PATH_CLI_BIN PATH_CLI_ALLOWLIST PATH_CLI_SHA256

    SH

    unless wrapper_content.include?("PATH_CLI_BIN:=#{opt_bin}/path")
      wrapper.atomic_write(wrapper_content.sub("#!/usr/bin/env sh\n", "#!/usr/bin/env sh\n\n#{defaults_block}"))
    end
  end

  def caveats
    <<~EOS
      Add this line to ~/.zshrc to enable shell integration:
        . "#{opt_pkgshare}/path-wrapper.sh"

      This formula is keg-only and does not link `path` into #{HOMEBREW_PREFIX}/bin.

      Then reload your shell:
        source ~/.zshrc
    EOS
  end

  test do
    (testpath/"sample.path").write <<~EOS
      # layout: '<location>' [<name>] (<options>)
      '/usr/local/bin' [localbin] (auto)
    EOS

    assert_predicate pkgshare/"path-wrapper.sh", :exist?

    script = <<~SH
      set -eu
      PATH="/usr/bin:/bin"
      HOME="#{testpath}"
      . "#{pkgshare}/path-wrapper.sh"
      path --file "#{testpath}/sample.path" list >/dev/null
    SH
    system "sh", "-c", script

    output = shell_output("#{bin}/path --file #{testpath}/sample.path list")
    assert_match "/usr/local/bin", output
    assert_match "localbin", output
  end
end
