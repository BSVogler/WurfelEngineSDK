package com.bombinggames.caveland.game;
/** Stub with the ids of the real CavelandBlocks.CLBlocks (checked against it by regenerate.sh). */
public class CavelandBlocks {
	public static enum CLBlocks {
		ENTRY((byte) 16), INDESTRUCTIBLEOBSTACLE((byte) 17), SULFUR((byte) 42), IRONORE((byte) 43), COAL((byte) 44);
		private final byte id;
		private CLBlocks(byte id) { this.id = id; }
		public final byte getId() { return id; }
	}
}
