package fixtures;

public enum Color {
    RED, GREEN, BLUE;

    public Color next() {
        return values()[(ordinal() + 1) % values().length];
    }
}
