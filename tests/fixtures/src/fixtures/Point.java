package fixtures;

import java.util.List;

public record Point(int x, int y, List<String> tags) {
    public Point {
        if (x < 0) throw new IllegalArgumentException("x");
    }
}
