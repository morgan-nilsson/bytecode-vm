package legacy;

public class Legacy {
    private int x;

    public int get() {
        synchronized (this) {
            return x;
        }
    }

    public Runnable task() {
        return new Runnable() {
            public void run() {
                x++;
            }
        };
    }
}
