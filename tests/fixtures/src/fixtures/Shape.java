package fixtures;

public sealed interface Shape permits Shape.Circle, Shape.Square {
    double area();

    default String describe() {
        return getClass().getSimpleName() + " " + area();
    }

    static Shape unit() {
        return new Square(1);
    }

    private static double square(double d) {
        return d * d;
    }

    final class Circle implements Shape {
        final double r;
        Circle(double r) { this.r = r; }
        public double area() { return Math.PI * square(r); }
    }

    non-sealed class Square implements Shape {
        final double side;
        Square(double side) { this.side = side; }
        public double area() { return square(side); }
    }
}
