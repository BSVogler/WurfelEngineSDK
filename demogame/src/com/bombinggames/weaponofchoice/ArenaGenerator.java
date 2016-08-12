package com.bombinggames.weaponofchoice;

import com.bombinggames.wurfelengine.core.map.Generator;
import java.util.Random;

/**
 *
 * @author Benedikt Vogler
 */
public class ArenaGenerator implements Generator {

	private long seed = 0;
	private Random generator;

	@Override
	public int generate(int x, int y, int z) {
		//initailize seed
		if (seed == 0) {
			seed = (long) (Math.random()*Long.MAX_VALUE);
			generator = new Random(seed);
		}

		if (z == 0) {//ground level covered with sand
			return 8;
		} else if (z == 1 && getRandom(x, y, z) < 0.05f) { //every twentiest block is a pillar 
			return 2;
		} else {
			if (z == 2 && generate(x, y, z - 1) != 0 && generate(x, y, z - 1) == 2) {
				return 1;
			} else {
				return 0;
			}
		}
	}

	/**
	 * Returns a random number for each field using the seed.
	 *
	 * @param x
	 * @param y
	 * @param z
	 * @return
	 */
	private float getRandom(int x, int y, int z) {
		//generate hash
		int field = x * y * z;//fastes way to generate id for every coodinate

		generator.setSeed(seed);
		float output = 0;
		for (int i = 0; i < field; i++) {
			output = generator.nextFloat();
		}

		return output;

	}

	@Override
	public void spawnEntities(int x, int y, int z) {
	}
}
