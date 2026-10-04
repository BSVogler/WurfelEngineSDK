package com.bombinggames.wurfelengine.core.map;
/** Stub: just the three integers. */
public class Coordinate {
	private final int x, y, z;
	public Coordinate(int x, int y, int z) { this.x = x; this.y = y; this.z = z; }
	public int getX() { return x; }
	public int getY() { return y; }
	public int getZ() { return z; }
	public Point toPoint() { return new Point(x, y, z); }
}
