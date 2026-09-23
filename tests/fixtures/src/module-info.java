module test.fixtures {
    requires java.logging;
    requires transitive java.sql;
    requires static java.desktop;
    exports fixtures;
    opens fixtures.internal to java.base;
    uses java.lang.Runnable;
    provides java.lang.Runnable with fixtures.internal.Task;
}
