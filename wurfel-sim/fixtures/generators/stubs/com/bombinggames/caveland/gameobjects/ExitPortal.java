package com.bombinggames.caveland.gameobjects;
import com.bombinggames.wurfelengine.core.map.Coordinate;
import com.bombinggames.wurfelengine.core.map.Point;
import java.util.ArrayList;
import java.util.List;
/** Stub that records what the generator spawns. */
public class ExitPortal {
	public static final List<String> SPAWNED = new ArrayList<>();
	private Point at;
	private boolean enemySpawner;
	public ExitPortal spawn(Point p) { at = p; return this; }
	public void enableEnemySpawner() { enemySpawner = true; }
	public void setTarget(Coordinate c) {
		SPAWNED.add("ExitPortal at " + at.x + " " + at.y + " " + at.z + " target " + c.getX() + " " + c.getY() + " " + c.getZ() + " enemy_spawner " + enemySpawner);
	}
}
