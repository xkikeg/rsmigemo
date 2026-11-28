#!/bin/sh
# Compare rsmigemo's output with cmigemo's, byte for byte.
#
# usage: scripts/compare-with-cmigemo.sh <cmigemo-binary> <migemo-dict> [extra cmigemo flags...]
#
# Generates a query corpus (all 1-3 letter romaji strings, mixed-case and
# symbol cases, and random syllable sequences), runs both programs in quiet
# mode, and prints the first differences. Exits 1 if any output differs.
#
# Expect about 20 differences per syntax: rsmigemo escapes character classes
# per regex flavor, where cmigemo can write broken classes such as
# `[\+-\.]` (see CMIGEMO.md). Every other query should match byte for byte.
set -eu
cmigemo=$1; dict=$2; shift 2
here=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT

cargo build --quiet --release --manifest-path "$here/Cargo.toml"
rsmigemo=${CARGO_TARGET_DIR:-$here/target}/release/rsmigemo

perl -e '
  my @a = ("a".."z");
  print "$_\n" for @a;
  for my $x (@a) { print "$x$_\n" for @a }
  for my $x (@a) { for my $y (@a) { print "$x$y$_\n" for @a } }
  print "$_\n" for qw(kensaku kensakuEngine ABCdef AB1 aB Aka KANJI nihongo
    a+b?c.d (a) [x] a|b ^\$ \\ / . かな カナ 漢字 ｋａｎａ ｶﾅ kakikukeko
    shinbun kyouto toukyou nn n\x27 xtu ltu kka tta ppa ssha chotto Nihon nIHON);
  srand(42);
  my @s = qw(a i u e o ka ki ku ke ko sa shi su se so ta chi tsu te to na ni nu ne no
    ha hi fu he ho ma mi mu me mo ya yu yo ra ri ru re ro wa n ga gi gu ge go za ji zu
    ze zo da de do ba bi bu be bo pa pi pu pe po kya kyu kyo sha shu sho cha chu cho
    nya nyu nyo rya ryu ryo gya gyu gyo ja ju jo xtu);
  for (1..5000) { print join("", map { $s[rand @s] } 1..(1 + int rand 4)), "\n" }
' > "$work/queries"

"$cmigemo" -q -d "$dict" "$@" < "$work/queries" > "$work/c.out"
"$rsmigemo" -q -d "$dict" "$@" < "$work/queries" > "$work/rs.out"

total=$(wc -l < "$work/queries")
paste "$work/queries" "$work/c.out" "$work/rs.out" | awk -F'\t' '$2 != $3' > "$work/diff"
bad=$(wc -l < "$work/diff")
echo "$total queries, $bad differ ($*)"
head -5 "$work/diff" | awk -F'\t' '{ printf "query: %s\n  c:  %s\n  rs: %s\n", $1, $2, $3 }'
[ "$bad" -eq 0 ]
