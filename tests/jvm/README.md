# Cross-checking the tests against a real JVM

The tests assert what the JVM specification requires. `CrossCheck` checks those
expectations against what HotSpot actually does, which is how the tests
themselves get validated.

```sh
CLASSFILE_TEST_DUMP=$PWD/target/dump cargo test
javac -d target/crosscheck tests/jvm/CrossCheck.java
java -cp target/crosscheck CrossCheck target/dump/manifest.txt
```

Every class a test builds is written to `target/dump`, with the expected
outcome recorded in `manifest.txt`. `CrossCheck` loads each one with
`ClassLoader.defineClass` and reports the disagreements.

Some disagreement is expected and fine:

* **Newer versions.** A JDK cannot load a class file newer than itself, so the
  tests covering major versions above that JDK's own will show as disagreeing.
* **Annotation contents.** JVMS 4.7.16 constrains them, but HotSpot does not
  validate them at definition time.
* **StackMapTable contents.** HotSpot keeps the attribute as raw bytes at
  definition time and only interprets it during verification, so a bad frame
  type or verification tag is not caught by `defineClass`. JVMS 4.7.4 defines
  those values, so the tests stay stricter.
* **Attributes in the wrong location.** This parser rejects a recognised
  attribute that turns up somewhere JVMS 4.7 does not permit it — a
  `ModuleMainClass` on an ordinary class, say. HotSpot ignores it instead.
* **module-info.** `defineClass` refuses every module-info outright, so those
  are skipped rather than compared.

As of the last run: **422 agree, 22 disagree, 53 skipped**. All 22 fall into
the categories above — 2 class files at version 69, 9 annotation and type
annotation checks, 10 StackMapTable checks, and 1 misplaced module attribute.
