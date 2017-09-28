package com.bombinggames.wurfelengine.core.map;

import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;

/**
 * An interface which is used to get information about custom blocks.
 *
 * @author Benedikt Vogler
 */
public class CustomBlocks {

	/**
	 * When it is possible to see though the sides.
	 * @param spriteId
	 * @param spriteValue
	 * @return
	 */
	public boolean isTransparent(byte spriteId, byte spriteValue){
		if (spriteId==0 || spriteId == 9 || spriteId == 4) {
			return true;
		}
		return false;
	}

	/**
	 * Check if the block is liquid.
	 *
	 * @param id
	 * @param value
	 * @return true if liquid, false if not
	 */
	public boolean isLiquid(byte id, byte value){
		return id == 9;
	}

	/**
	 * Default is "undefined".
	 *
	 * @param id
	 * @param value
	 * @return
	 */
	public String getName(byte id, byte value){
		return "undefined";
	}

	/**
	 *
	 * @param id there are ids &lt; 10 which are filtered before
	 * @param value
	 * @return
	 */
	public RenderCell toRenderBlock(byte id, byte value){
		return new RenderCell(id, value);
	}

	/**
	 * Is the block a true block with three sides or does it get rendered by a
	 * single sprite?<br>
	 * This field is only used for representation (view) related data.<br>
	 * Only used for blocks. Entities should return <i>false</i>.
	 *
	 * @param spriteId
	 * @param spriteValue
	 * @return <i>true</i> if it has sides, <i>false</i> if is rendered as a
	 * single sprite
	 */
	public boolean hasSides(byte spriteId, byte spriteValue) {
		return !(spriteId == 0 || spriteId == 4);
	}

	/**
	 *
	 * @param id
	 * @param value
	 * @return
	 */
	public boolean isIndestructible(byte id, byte value){
		return false;
	}

	/**
	 * Engine reserves ids are 0-9 and must be redirected to return super
	 * method.
	 *
	 * @param id
	 * @param value
	 * @return
	 */
	public boolean isObstacle(byte id, byte value) {
		if (id == 9) {
			return false;
		}

		return id != 0;
	}

	/**
	 *
	 * @param block
	 * @return
	 */
	public boolean isObstacle(int block) {
		return isObstacle((byte) (block & 255), (byte) ((block >> 8) & 255));
	}
}
