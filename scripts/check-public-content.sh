#!/usr/bin/env bash
# Blocks private content from entering this public repository.
#
#   scripts/check-public-content.sh            # every tracked or new, non-ignored file
#   scripts/check-public-content.sh --staged   # staged files (used by .githooks/pre-commit)
#   scripts/check-public-content.sh FILE...    # the given files
#
# Private names that must never appear (clients, other products, hosts, user names) go in the
# untracked file `.private-patterns` at the repository root: one Perl regex per line, matched
# case-insensitively; blank lines and lines starting with # are ignored. That file is
# gitignored, so the list itself is never published. Needs only git, bash and perl.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
cd "$root"

list=$(mktemp)
trap 'rm -f "$list"' EXIT
if [[ "${1:-}" == --staged ]]; then
  git diff --cached --name-only --diff-filter=ACMR -z > "$list"
elif (($#)); then
  printf '%s\0' "$@" > "$list"
else
  git ls-files -co --exclude-standard -z > "$list"
fi

exec perl -CS -e '
use strict; use warnings; use Encode qw(decode FB_DEFAULT);
my @checks = (
  ["Local absolute path (use repo-relative paths, \$HOME, \$TMPDIR or a placeholder)",
   qr{/Users/[A-Za-z0-9._-]+/|/home/[a-z0-9._-]+/|/Volumes/[A-Za-z0-9]|/var/folders/[a-z0-9]}],
  ["Real Tailscale host name (use <machine>.<tailnet>.ts.net)",
   qr/\b[a-z0-9-]+\.tail[0-9a-f]{4,}\.ts\.net\b/i],
  ["Secret or private key",
   qr/-----BEGIN [A-Z ]*PRIVATE KEY-----|\bgh[pousr]_[A-Za-z0-9]{20,}|\bgithub_pat_[A-Za-z0-9_]{20,}|\bAKIA[0-9A-Z]{16}\b|\bsk-[A-Za-z0-9_-]{20,}|\bxox[abpr]-[A-Za-z0-9-]{10,}/],
);
my $email = qr/[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/;
my $allowed = qr/@(?:[0-9]x\.|(?:[A-Za-z0-9-]+\.)*example\.[A-Za-z.]+$|[A-Za-z0-9.-]+\.(?:invalid|test|example|localhost)$|latofonts\.com$)/;
if (open my $p, "<:encoding(UTF-8)", ".private-patterns") {
  my @own = grep { !/^\s*(?:#|$)/ } map { s/\s+$//r } <$p>;
  push @checks, ["Private name listed in .private-patterns", qr/(?i:@{[join "|", map { "(?:$_)" } @own]})/] if @own;
}
my $skip = qr/\.(?:png|gif|jpe?g|webp|ico|ttf|otf|woff2?|wasm|pdf|zip)$|(?:^|\/)(?:Cargo\.lock|package-lock\.json)$/;
my (%hits, $count);
local $/ = "\0";
open my $list, "<", $ARGV[0] or die "list: $!";
while (my $file = <$list>) {
  chomp $file;
  next if $file =~ $skip or !-f $file;
  open my $fh, "<:raw", $file or next;
  my $head = ""; read $fh, $head, 8192; next if $head =~ /\0/; seek $fh, 0, 0;
  local $/ = "\n";
  while (my $raw = <$fh>) {
    my $line = decode("UTF-8", $raw, FB_DEFAULT); chomp $line;
    for my $c (@checks) {
      push @{$hits{$c->[0]}}, "$file:$.: " . substr($line, 0, 160) if $line =~ $c->[1];
    }
    for my $m ($line =~ /($email)/g) {
      push @{$hits{"Email address (only example/test domains are allowed)"}}, "$file:$.: $m" unless $m =~ $allowed;
    }
  }
}
for my $label (sort keys %hits) {
  my @rows = @{$hits{$label}}; $count += @rows;
  print STDERR "check-public-content: $label\n", map({ "  $_\n" } @rows[0 .. ($#rows < 39 ? $#rows : 39)]),
    (@rows > 40 ? "  ... " . (@rows - 40) . " more\n" : ""), "\n";
}
if ($count) { print STDERR "check-public-content: blocked. Remove the private data above (see AGENTS.md, \"Public repository\").\n"; exit 1 }
' "$list"
