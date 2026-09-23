import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Loads every class file dumped by the C tests (see classfile_support.h) with
 * ClassLoader.defineClass and compares the JVM's verdict with the test's
 * expectation. Disagreements usually mean a test expects something the JVM
 * doesn't enforce (or vice versa) and deserve a second look.
 *
 * Usage: java CrossCheck build/crosscheck/dump/manifest.txt
 */
public class CrossCheck {
    static final class Loader extends ClassLoader {
        Class<?> define(byte[] b) {
            return defineClass(null, b, 0, b.length);
        }
    }

    public static void main(String[] args) throws Exception {
        Map<String, String> expected = new LinkedHashMap<>();
        for (String line : Files.readAllLines(Path.of(args[0]))) {
            int space = line.indexOf(' ');
            if (space > 0) expected.put(line.substring(space + 1), line.substring(0, space));
        }

        int agree = 0, disagree = 0, skipped = 0;
        for (Map.Entry<String, String> e : expected.entrySet()) {
            Path path = Path.of(e.getKey());
            String verdict;
            String detail = "";
            try {
                new Loader().define(Files.readAllBytes(path));
                verdict = "accept";
            } catch (ClassFormatError err) {
                verdict = "reject";
                detail = err.toString();
            } catch (NoClassDefFoundError err) {
                String msg = String.valueOf(err.getMessage());
                if (msg.contains("ACC_MODULE")) {
                    skipped++; // defineClass refuses every module-info
                    continue;
                }
                // A missing superclass or interface: the format itself was accepted.
                verdict = "accept";
                detail = err.toString();
            } catch (SecurityException err) {
                skipped++; // e.g. defining java/lang/Object
                continue;
            } catch (LinkageError err) {
                verdict = "reject";
                detail = err.toString();
            }

            if (verdict.equals(e.getValue())) {
                agree++;
            } else {
                disagree++;
                String name = path.getFileName().toString();
                System.out.printf("test expects %-6s JVM says %-6s %s%n", e.getValue(), verdict, name);
                if (!detail.isEmpty()) System.out.printf("    %s%n", detail);
            }
        }
        System.out.printf("%d agree, %d disagree, %d skipped%n", agree, disagree, skipped);
    }
}
