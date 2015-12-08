package com.bombinggames.weaponofchoice;

import com.bombinggames.wurfelengine.core.Gameobjects.Block;
import com.bombinggames.wurfelengine.core.Map.Generator;
import java.util.Random;

/**
 *
 * @author Benedikt Vogler
 */
public class ArenaGenerator implements Generator {

	private double seed = 0;

	@Override
	public Block generate(int x, int y, int z) {
		if (seed == 0) {
			seed = Math.random();
		}

		if (z == 0) {
			return Block.getInstance((byte) 8);
		} else {
			if (z == 1 && getRandom(x, y, z) < 0.05f) { //ever twentiest block is a pillar 
				return Block.getInstance((byte) 2);
			}
			if (z == 2 && generate(x, y, z - 1) != null && generate(x, y, z - 1).getId() == 2) {
				return Block.getInstance((byte) 1);
			} else {
				return null;
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
		//bet

		Random generator = new Random((long) seed);
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
