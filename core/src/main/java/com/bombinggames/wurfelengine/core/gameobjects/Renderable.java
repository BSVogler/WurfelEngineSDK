package com.bombinggames.wurfelengine.core.gameobjects;

import com.bombinggames.wurfelengine.core.Camera;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.map.Coordinate;
import com.bombinggames.wurfelengine.core.map.Point;
import com.bombinggames.wurfelengine.core.map.Position;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import com.bombinggames.wurfelengine.core.map.rendering.RenderStorage;
import java.util.LinkedList;

/**
 * Interface for objects in the game world whether they are blocks or entities.
 *
 * @author Benedikt Vogler
 */
public interface Renderable {

	/**
	 * How bright is the object? The lightlevel is a scale applied to the color.
	 * 1 is default value.
	 *
	 * @return 1 is default bright. 0 is black.
	 */
	float getLightlevelR();

	/**
	 * How bright is the object? The lightlevel is a scale applied to the color.
	 * 1 is default value.
	 *
	 * @return 1 is default bright. 0 is black.
	 */
	float getLightlevelG();

	/**
	 * How bright is the object? The lightlevel is a scale applied to the color.
	 * 1 is default value.
	 *
	 * @return 1 is default bright. 0 is black.
	 */
	float getLightlevelB();

	/**
	 * Set the brightness of the object. The lightlevel is a scaling factor. 1
	 * is default value.
	 *
	 * @param lightlevel 1 is default bright. 0 is black.
	 */
	void setLightlevel(float lightlevel);

	/**
	 * Draws an object if it is not hidden and not clipped.
	 *
	 * @param view the view using this render method
	 * @param camera The camera rendering the scene
	 */
	public void render(GameView view, Camera camera);

	/**
	 * Return the coordinates of the object in the game world. Not copy safe as it points to the interaly used object.
	 *
	 * @return Reference to the position object which points to the location in
	 * the game world.
	 * @see #getPoint()
	 */
	public Position getPosition();

	/**
	 * not copy save
	 *
	 * @return
	 * @see #getPosition()
	 */
	public Point getPoint();

	/**
	 * not copy save
	 *
	 * @return
	 * @see #getPosition()
	 */
	public Coordinate getCoord();

	/**
	 * Gives information if object should be rendered.
	 *
	 * @param camera
	 * @return
	 */
	public boolean shouldBeRendered(Camera camera);

	/**
	 * get the blocks which must be rendered before
	 *
	 * @param rs
	 * @return
	 */
	public LinkedList<RenderCell> getCoveredBlocks(RenderStorage rs);
}
