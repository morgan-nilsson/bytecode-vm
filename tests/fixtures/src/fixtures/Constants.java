package fixtures;

// Lots of wide constants so Long/Double two-slot handling is exercised throughout the pool.
public final class Constants {
    public static final long L0 = 0x0123456789ABCDEFL;
    public static final double D0 = Double.MIN_VALUE;
    public static final long L1 = Long.MIN_VALUE;
    public static final double D1 = Double.MAX_VALUE;
    public static final long L2 = Long.MAX_VALUE;
    public static final double D2 = Double.NEGATIVE_INFINITY;
    public static final double D3 = Double.NaN;
    public static final float F0 = Float.MIN_VALUE;
    public static final float F1 = -0.0f;
    public static final int I0 = Integer.MIN_VALUE;
    // e-acute, euro sign, an emoji (surrogate pair) and NUL, which modified UTF-8 encodes as C0 80.
    public static final String UNICODE = "h\u00e9llo \u20ac \uD83D\uDE00 \u0000 end";

    public static long mix(long a, double b) {
        return a ^ Double.doubleToLongBits(b) ^ 0x7FEDCBA987654321L;
    }
}
