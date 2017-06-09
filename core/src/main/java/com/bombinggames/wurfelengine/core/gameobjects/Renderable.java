/*
 * If this software is used for a game the official „Wurfel Engine“ logo or its name must be visible in an intro screen or main menu.
 *
 * Copyright 2017 Benedikt Vogler.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * * Redistributions of source code must retain the above copyright notice, 
 *   this list of conditions and the following disclaimer.
 * * Redistributions in binary form must reproduce the above copyright notice, 
 *   this list of conditions and the following disclaimer in the documentation 
 *   and/or other materials provided with the distribution.
 * * Neither the name of Benedikt Vogler nor the names of its contributors 
 *   may be used to endorse or promote products derived from this software without specific
 *   prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
 * AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
 * LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
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
//maybe should be merged with abstract game object
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
	 * @param camera The camera rendering the scene
	 */
	public void render(GameView camera);

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
