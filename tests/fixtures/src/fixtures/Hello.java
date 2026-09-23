package fixtures;

import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import java.util.function.IntSupplier;

public class Hello implements Runnable {
    public static final int INT_CONST = 42;
    public static final long LONG_CONST = 1234567890123L;
    public static final float FLOAT_CONST = 3.5f;
    public static final double DOUBLE_CONST = 2.718281828459045;
    public static final String STRING_CONST = "hello, class file";
    private static final long NEGATIVE_LONG = -9223372036854775807L;

    private final List<String> names = new ArrayList<>();
    protected volatile int counter;
    transient Object cache;

    public Hello() {}

    public static void main(String[] args) throws IOException, InterruptedException {
        Hello h = new Hello();
        h.run();
        System.out.println("sum = " + h.sum(10));
    }

    @Override
    public void run() {
        names.add("a");
        IntSupplier s = () -> counter + names.size();
        counter = s.getAsInt();
    }

    public int sum(int n) {
        int total = 0;
        for (int i = 0; i < n; i++) {
            if (i % 2 == 0) {
                total += i;
            } else {
                long wide = i * 3L;
                total -= (int) wide;
            }
        }
        return total;
    }

    public synchronized <T extends Comparable<T>> T max(List<T> items) {
        T best = null;
        for (T item : items) {
            if (best == null || item.compareTo(best) > 0) best = item;
        }
        return best;
    }

    public String tryCatch(String input) {
        try {
            return Integer.toString(Integer.parseInt(input));
        } catch (NumberFormatException | NullPointerException e) {
            return "bad";
        } finally {
            counter++;
        }
    }

    @Deprecated
    public static native void nativeMethod(double d, long l);

    public int anonymous() {
        IntSupplier s = new IntSupplier() {
            @Override
            public int getAsInt() {
                return counter;
            }
        };
        return s.getAsInt();
    }

    public class Inner {
        int value() { return counter; }
    }

    static class Nested {
        private int secret() { return 1; }
    }

    public int varargs(int... values) {
        return values.length;
    }

    public String switchOnString(String s) {
        switch (s) {
            case "one": return "1";
            case "two": return "2";
            default: return "?";
        }
    }
}
