package fixtures;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;
import java.util.List;

@Anno(i = 99, str = "on class", color = Color.BLUE, ints = {7}, type = String.class)
@Annotated.Invisible
public class Annotated {
    @Retention(RetentionPolicy.CLASS)
    @interface Invisible {}

    @Retention(RetentionPolicy.RUNTIME)
    @Target({ElementType.TYPE_USE, ElementType.TYPE_PARAMETER})
    @interface TypeUse {}

    @Target(ElementType.TYPE_USE)
    @interface InvisibleTypeUse {}

    @Anno(z = false)
    public @TypeUse String field;

    public List<@TypeUse @InvisibleTypeUse String> typed;

    @Anno
    public <@TypeUse T> void method(@Anno(j = 10) int a, @Invisible String b, final @TypeUse Object c)
            throws @TypeUse RuntimeException {
        @TypeUse Object local = (@TypeUse Object) c;
        if (local instanceof @TypeUse String) {
            field = b;
        }
    }
}
