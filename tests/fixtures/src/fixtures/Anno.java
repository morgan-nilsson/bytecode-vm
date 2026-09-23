package fixtures;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

@Retention(RetentionPolicy.RUNTIME)
@Target({ElementType.TYPE, ElementType.FIELD, ElementType.METHOD, ElementType.PARAMETER})
public @interface Anno {
    byte b() default 1;
    char c() default 'c';
    short s() default 2;
    int i() default 3;
    long j() default 4L;
    float f() default 5.0f;
    double d() default 6.0;
    boolean z() default true;
    String str() default "default";
    Color color() default Color.GREEN;
    Class<?> type() default Object.class;
    Retention nested() default @Retention(RetentionPolicy.CLASS);
    int[] ints() default {1, 2, 3};
    String[] empty() default {};
}
