package com.bombinggames.minecrafttowurfelengine.mcmodify.location;

import com.bombinggames.minecrafttowurfelengine.mcmodify.minecraft.Region;

/**
 * Location of a Chunk in a Region.
 */
public class LocChunkInRegion {

	public final int x;
	public final int z;

	public LocChunkInRegion(int chunkX, int chunkZ) {
		if (chunkX < 0 || chunkX >= 32 || chunkZ < 0 || chunkZ >= 32) {
			throw new IllegalArgumentException("Invalid chunk location (" + chunkX + "," + chunkZ + ") in region ");
		}
		x = chunkX;
		z = chunkZ;
	}

	/**
	 * Returns the location of this chunk in a dimension.
	 *
	 * @param loc The region this chunk is in.
	 * @return The location of this chunk in a dimension.
	 */
	public LocChunkInDimension getLocInDimension(LocRegionInDimension loc) {
		return new LocChunkInDimension(loc.x * Region.CHUNK_X_SIZE + x, loc.z * Region.CHUNK_Z_SIZE + z);
	}
}
