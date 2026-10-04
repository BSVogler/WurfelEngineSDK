import com.bombinggames.caveland.game.ChunkGenerator;
import com.bombinggames.caveland.gameobjects.ExitPortal;
import com.bombinggames.wurfelengine.core.map.Coordinate;
import com.bombinggames.wurfelengine.core.map.Generator;
import com.bombinggames.wurfelengine.core.map.Generators.*;
import com.bombinggames.weaponofchoice.ArenaGenerator;
import java.io.*;
import java.lang.reflect.Field;
import java.util.Random;

/** Runs the real, unmodified Java generators and writes what they return as test fixtures. */
public class Harness {
	static PrintStream out;

	static void open(String dir, String name) throws Exception {
		if (out != null) out.close();
		out = new PrintStream(new FileOutputStream(new File(dir, name)), false, "UTF-8");
	}

	/** One line per column: "x y: b0 b1 ... bn" with the raw int the generator returns. */
	static void column(Generator g, int x, int y, int zMax) {
		StringBuilder sb = new StringBuilder().append(x).append(' ').append(y).append(':');
		for (int z = 0; z <= zMax; z++) sb.append(' ').append(g.generate(x, y, z));
		out.println(sb);
	}

	static void setField(Object o, String name, Object value) throws Exception {
		Field f = o.getClass().getDeclaredField(name);
		f.setAccessible(true);
		f.set(o, value);
	}

	public static void main(String[] args) throws Exception {
		String dir = args[0];

		// IslandGenerator: the peak is random in Java; pin it to compare the formula.
		open(dir, "island.txt");
		int[][] peaks = {{3, 17}, {0, 0}, {9, 39}};
		for (int[] p : peaks) {
			IslandGenerator g = new IslandGenerator();
			setField(g, "mountainX", p[0]);
			setField(g, "mountainY", p[1]);
			out.println("peak " + p[0] + " " + p[1]);
			for (int x = -3; x <= 14; x++) for (int y = -3; y <= 30; y++) column(g, x, y, 9);
		}

		open(dir, "air.txt");
		for (int x = -2; x <= 2; x++) for (int y = -2; y <= 2; y++) column(new AirGenerator(), x, y, 9);

		open(dir, "blocktest.txt");
		Generator bt = new BlockTestGenerator();
		for (int y = -130; y <= 260; y++) column(bt, 0, y, 2);
		column(bt, 5, Integer.MIN_VALUE, 1);
		column(bt, 5, Integer.MAX_VALUE, 1);

		open(dir, "fullmap.txt");
		for (int id : new int[]{0, 1, 2, 7, 9, 127, 128, 200, 255}) {
			out.println("id " + id);
			column(new FullMapGenerator((byte) id), 3, -4, 9);
		}

		// ArenaGenerator picks its seed with Math.random(); pin it through reflection.
		open(dir, "arena.txt");
		for (long seed : new long[]{1L, 42L, -7L, 123456789012345L}) {
			ArenaGenerator g = new ArenaGenerator();
			setField(g, "seed", seed);
			setField(g, "generator", new Random(seed));
			out.println("seed " + seed);
			for (int x = -6; x <= 30; x++) for (int y = -6; y <= 30; y++) column(g, x, y, 3);
		}

		// java.util.Random itself, for the Rust reimplementation.
		open(dir, "random.txt");
		for (long seed : new long[]{0L, 42L, -1L, 123456789012345L}) {
			Random r = new Random(seed);
			out.println("seed " + seed + " nextInt " + r.nextInt());
			r = new Random(seed);
			StringBuilder sb = new StringBuilder("floats");
			for (int i = 0; i < 40; i++) sb.append(' ').append(Float.floatToIntBits(r.nextFloat()));
			out.println(sb);
		}

		// Caveland: its own main() ASCII picture, captured exactly.
		open(dir, "caveland_ascii.txt");
		PrintStream real = System.out;
		System.setOut(out);
		ChunkGenerator.main(new String[0]);
		System.setOut(real);

		ChunkGenerator cl = new ChunkGenerator();
		open(dir, "caveland_overworld.txt");
		for (int x = -5; x <= 5; x++) for (int y : new int[]{-3, 0, 1, 998, 999, 1000, 1001, 1100, 1199}) column(cl, x, y, 6);

		open(dir, "caveland_underworld.txt");
		for (int x = -3; x <= 60; x++) for (int y = 1198; y <= 1232; y++) column(cl, x, y, 6);
		// a few far away columns, including negative x
		for (int x : new int[]{-1000, -28, 100, 5000}) for (int y = 1200; y <= 1215; y++) column(cl, x, y, 6);

		open(dir, "caveland_inside_outside.txt");
		for (int x = -30; x <= 60; x++) for (int y = 1195; y <= 1240; y++) {
			StringBuilder sb = new StringBuilder().append(x).append(' ').append(y).append(':');
			for (int z : new int[]{2, 3, 4, 5}) sb.append(' ').append(ChunkGenerator.insideOutside(x, y, z));
			out.println(sb);
		}

		open(dir, "caveland_spawns.txt");
		for (int x = -30; x <= 130; x++) for (int y = 1190; y <= 1300; y++) for (int z = 0; z <= 8; z++) cl.spawnEntities(x, y, z);
		for (String s : ExitPortal.SPAWNED) out.println(s);

		open(dir, "caveland_helpers.txt");
		for (int n = -2; n <= 8; n++) {
			Coordinate up = ChunkGenerator.getCaveUp(n), down = ChunkGenerator.getCaveDown(n), c = ChunkGenerator.getCaveCenter(n);
			out.println("cave " + n + " up " + up.getX() + " " + up.getY() + " " + up.getZ()
				+ " down " + down.getX() + " " + down.getY() + " " + down.getZ()
				+ " center " + c.getX() + " " + c.getY() + " " + c.getZ());
		}
		for (int x = -60; x <= 140; x += 3) for (int y : new int[]{1199, 1200, 1250}) out.println("number " + x + " " + y + " " + ChunkGenerator.getCaveNumber(x, y, 0));
		out.close();
	}
}
