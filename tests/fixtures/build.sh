#!/bin/sh
# Regenerates tests/fixtures/classes from tests/fixtures/src.
#
# The .class files are committed so the tests don't need a JDK. The fixture
# tests check exact values, so if you regenerate with a different javac
# expect to update tests/test_classfile_fixtures.c.
#
# Needs JDK 17+ (javac, jar).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
src="$here/src"
out="$here/classes"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

rm -rf "$out"
mkdir -p "$out"

# Java 17 classes with full debug info and parameter names.
modular_sources=$(find "$src/fixtures" "$src/module-info.java" -name '*.java')
javac -g -parameters --release 17 -d "$tmp/modular" $modular_sources

# `jar` adds the ModulePackages and ModuleMainClass attributes to module-info.
jar --create --file "$tmp/fixtures.jar" --main-class fixtures.Hello -C "$tmp/modular" .
mkdir -p "$tmp/jar"
(cd "$tmp/jar" && jar --extract --file "$tmp/fixtures.jar")
cp -R "$tmp/jar/fixtures" "$out/"
cp "$tmp/jar/module-info.class" "$out/"

# Older class file versions.
javac -g:none --release 8 -d "$tmp/release8" "$src/legacy/Legacy.java"
cp "$tmp/release8/legacy/Legacy.class" "$out/Legacy_release8.class"
javac -g:none --release 7 -Xlint:-options -d "$tmp/release7" "$src/legacy/Legacy.java" 2>/dev/null ||
    echo "note: this javac can't target release 7; skipping Legacy_release7.class" >&2
if [ -f "$tmp/release7/legacy/Legacy.class" ]; then
    cp "$tmp/release7/legacy/Legacy.class" "$out/Legacy_release7.class"
fi

find "$out" -name '*.class' | sort
