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

import com.badlogic.gdx.Gdx;
import com.badlogic.gdx.graphics.Color;
import com.badlogic.gdx.graphics.Pixmap;
import com.badlogic.gdx.graphics.Texture;
import com.badlogic.gdx.graphics.g2d.Sprite;
import com.badlogic.gdx.graphics.g2d.TextureAtlas;
import com.badlogic.gdx.graphics.g2d.TextureAtlas.AtlasRegion;
import com.badlogic.gdx.graphics.g3d.Renderable;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.Camera;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.map.Coordinate;
import com.bombinggames.wurfelengine.core.map.Point;
import com.bombinggames.wurfelengine.core.map.Position;
import com.bombinggames.wurfelengine.core.map.rendering.GameSpaceSprite;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import static com.bombinggames.wurfelengine.core.map.rendering.RenderCell.VIEW_HEIGHT2;
import com.bombinggames.wurfelengine.core.map.rendering.RenderStorage;
import java.io.FileNotFoundException;
import java.io.Serializable;
import java.util.LinkedList;

/**
 * An AbstractGameObject is something wich can be found in the game world.
 *
 * @author Benedikt
 */
public abstract class AbstractGameObject extends Renderable implements Serializable {

	private transient static final long serialVersionUID = 2L;

	/**
	 * The sprite texture which contains every object texture
	 */
	private transient static TextureAtlas spritesheet;
	private transient static String spritesheetPath = "com/bombinggames/wurfelengine/core/images/spritesheet";
	private transient static Pixmap pixmap;
	/**
	 * indexed acces to the spritesheet
	 */
	private static final transient AtlasRegion[][][] sprites = new AtlasRegion['z'][RenderCell.OBJECTTYPESNUM][RenderCell.VALUESNUM];//{category}{id}{value}
	private static Texture textureDiff;
	private static Texture textureNormal;
	/**
	 * bit position = camera id
	 */
	private static int currentMarkedFlag;

	/**
	 * disposes static fields
	 */
	public static void staticDispose() {
		spritesheet.dispose();//is this line needed?
		WE.getAssetManager().unload(spritesheetPath + ".txt");
		spritesheet = null;
		//clear index
		for (AtlasRegion[][] type : sprites) {
			for (AtlasRegion[] id : type) {
				for (int i = 0; i < id.length; i++) {
					id[i] = null;
				}
			}
		}
		if (pixmap != null) {
			pixmap.dispose();
		}
		pixmap = null;
	}

	/**
	 * the diffuse map
	 *
	 * @return
	 */
	public static Texture getTextureDiffuse() {
		return textureDiff;
	}

	/**
	 * the normal map
	 *
	 * @return
	 */
	public static Texture getTextureNormal() {
		return textureNormal;
	}

	/**
	 *
	 * @return can be null if pixmap loadin is disabled
	 */
	public static Pixmap getPixmap() {
		return pixmap;
	}

	/**
	 * Set your custom spritesheet path. the suffix will be added
	 *
	 * @param customPath format like
	 * "com/bombinggames/wurfelengine/core/images/Spritesheet" without suffix
	 */
	public static void setCustomSpritesheet(String customPath) {
		AbstractGameObject.spritesheetPath = customPath;
	}

	/**
	 * path of the spritesheet
	 *
	 * @return
	 */
	public static String getSpritesheetPath() {
		return spritesheetPath;
	}

	/**
	 * Load the spritesheet from memory.
	 * @throws java.io.FileNotFoundException
	 */
	public static void loadSheet() throws FileNotFoundException {
		Gdx.app.log("AGameObject", "getting spritesheet");
		if (spritesheet == null) {
			//if not in asset manager, then load into it
			if (!WE.getAssetManager().isLoaded(spritesheetPath + ".txt")) {
				WE.getAssetManager().load(spritesheetPath + ".txt", TextureAtlas.class);
				WE.getAssetManager().finishLoadingAsset(spritesheetPath + ".txt");
			}
			spritesheet = WE.getAsset(spritesheetPath + ".txt");
		}
		textureDiff = spritesheet.getTextures().first();
		if (WE.getCVars().getValueB("LEnormalMapRendering")) {
			textureNormal = WE.getAsset(spritesheetPath + "Normal.png");
		}

		//load again for pixmap, allows access to image color data;
		if (WE.getCVars().getValueB("loadPixmap")) {
			if (pixmap == null) {
				//pixmap = WurfelEngine.getInstance().manager.get("com/bombinggames/Game/Blockimages/Spritesheet.png", Pixmap.class);
				pixmap = new Pixmap(
					Gdx.files.internal(spritesheetPath + ".png")
				);
			}
		}
	}

	/**
	 * Returns a sprite texture.
	 *
	 * @param category the category of the sprite e.g. 'b' for blocks
	 * @param id the id of the object
	 * @param value the value of the object
	 * @return
	 */
	public static AtlasRegion getSprite(final char category, final byte id, final byte value) {
		if (spritesheet == null || id <= 0 || value < 0) {
			return null;
		}
		if (sprites[category][id][value] == null) { //load if not already loaded
			AtlasRegion sprite = spritesheet.findRegion(category + Integer.toString(id) + "-" + value);
			if (sprite == null) { //if there is no sprite show the default "sprite not found sprite" for this category
				Gdx.app.debug("Spritesheet", category + Integer.toString(id) + "-" + value + " not found");
				sprite = spritesheet.findRegion(category + "0-0");
				if (sprite == null) {//load generic error sprite if category sprite failed
					sprite = spritesheet.findRegion("error");
					if (sprite == null) {
						throw new NullPointerException("Sprite and category error not found and even the generic error sprite could not be found. Something with the sprites is fucked up.");
					}
				}
			}
			sprites[category][id][value] = sprite;
			return sprite;
		} else {
			return sprites[category][id][value];
		}
	}

	//getter & setter
	/**
	 * Returns the spritesheet used for rendering.
	 *
	 * @return the spritesheet used by the objects
	 */
	public static TextureAtlas getSpritesheet() {
		return spritesheet;
	}

	/**
	 * inverses the dirty flag comparison so everything marked is now unmarked.
	 * used to mark the visited obejcts with depthsort.
	 * @param id
	 */
	public static void inverseMarkedFlag(int id) {
		currentMarkedFlag ^= 1 << id;
	}

	//render information
	private boolean hidden;
	private float rotation;
	private float scaling = 1;

	/**
	 * default is RGBA 0x808080FF.
	 */
	private transient Color tint = new Color(0.5f, 0.5f, 0.5f, 1f);
	/**
	 * flag used for depth sorting
	 */
	private int marked;
	/**
	 * caching the sprite for rendering
	 */
	protected transient GameSpaceSprite sprite;

	/**
	 * Creates an object.
	 *
	 */
	protected AbstractGameObject() {
	}

	/**
	 * Get the category letter for accessing sprites.
	 *
	 * @return
	 */
	public abstract char getSpriteCategory();

	/**
	 * The height of the object for depth sorting.
	 *
	 * @return game space
	 */
	public abstract int getDimensionZ();

	/**
	 * Set the coordinates without safety check. May use different object
	 * pointing to the same position.
	 *
	 * @param pos the coordinates you want to set
	 */
	public abstract void setPosition(Position pos);

	/**
	 * How bright is the object? The lightlevel is a scale applied to the color.
	 * 1 is default value.
	 *
	 * @return 1 is default bright. 0 is black.
	 */
	abstract public float getLightlevelR();

	/**
	 * How bright is the object? The lightlevel is a scale applied to the color.
	 * 1 is default value.
	 *
	 * @return 1 is default bright. 0 is black.
	 */
	abstract public float getLightlevelG();

	/**
	 * How bright is the object? The lightlevel is a scale applied to the color.
	 * 1 is default value.
	 *
	 * @return 1 is default bright. 0 is black.
	 */
	abstract public float getLightlevelB();

	/**
	 * Set the brightness of the object. The lightlevel is a scaling factor. 1
	 * is default value.
	 *
	 * @param lightlevel 1 is default bright. 0 is black.
	 */
	abstract public void setLightlevel(float lightlevel);

	/**
	 * Return the coordinates of the object in the game world. Not copy safe as
	 * it points to the interaly used object.
	 *
	 * @return Reference to the position object which points to the location in
	 * the game world.
	 * @see #getPoint()
	 */
	abstract public Position getPosition();

	/**
	 * Can be internal reference or shared object.
	 *
	 * @return
	 * @see #getPosition()
	 */
	abstract public Point getPoint();

	/**
	 * not copy save
	 *
	 * @return
	 * @see #getPosition()
	 */
	abstract public Coordinate getCoord();

	/**
	 * get the blocks which must be rendered before
	 *
	 * @param rs
	 * @return
	 */
	abstract public LinkedList<RenderCell> getCoveredBlocks(RenderStorage rs);
	
	/**
	 * Returns the depth of the object. The depth is the game world space
	 * projected on one axis orthogonal to the camera's angle.
	 * Objects nearer to camera have a bigger value.
	 *
	 * @return distance from zero level
	 */
	public float getDepth() {
		Point pos = getPoint();
		return pos.getY() + (pos.getZ() + getDimensionZ()) * RenderCell.PROJECTIONFACTORZ;//or Point.SQRT12?
	}

	/**
	 * Draws an object if it is not hidden and not clipped.
	 * in game space
	 * @param view
	 */
	public void render(GameView view) {
		byte id = getSpriteId();
		byte value = getSpriteValue();
		if (id > 0 && value >= 0 && !hidden && getPosition() != null) {
			if (sprite==null) {
				updateSpriteCache();
			}
			if (rotation != sprite.getRotation()) {
				sprite.setRotation(rotation);
			}
			//sprite.setOrigin(0, 0);
			if (scaling != sprite.getScaleX()) {
				sprite.setScale(scaling);
			}

			Point pos = getPoint();
			sprite.setPosition(
				pos.getX(),
				pos.getY()+RenderCell.GAME_DIAGLENGTH2,//center, move a bit to draw front
				pos.getZ()
			);

			//hack for transient field tint
			if (tint == null) {
				tint = new Color(0.5f, 0.5f, 0.5f, 1f);
			}
			sprite.setColor(tint);

			sprite.draw(view.getGameSpaceSpriteBatch());
		}
	}
	
	/**
	 * Renders at a custom position in projection space. uses heap
	 *
	 * @param view
	 * @param xPos rendering position, center of sprite in projection space
	 * @param yPos rendering position, center of sprite in projection space
	 */
	public void render(GameView view, int xPos, int yPos) {
		byte id = getSpriteId();
		byte value = getSpriteValue();
		if (id > 0 && value >= 0) {
			AtlasRegion texture = AbstractGameObject.getSprite(getSpriteCategory(), getSpriteId(), getSpriteValue());
			Sprite sprite = new Sprite(texture);
			sprite.setOrigin(
				texture.originalWidth / 2 - texture.offsetX,
				VIEW_HEIGHT2 - texture.offsetY
			);
			if (rotation != sprite.getRotation()) {
				sprite.setRotation(rotation);
			}
			
			if (scaling != sprite.getScaleX()) {
				sprite.setScale(scaling);
			}

			sprite.setPosition(
				xPos+texture.offsetX - texture.originalWidth / 2,
				yPos//center
				- VIEW_HEIGHT2
				+ texture.offsetY
			);

			//hack for transient field tint
			if (tint == null) {
				tint = new Color(0.5f, 0.5f, 0.5f, 1f);
			}
			sprite.setColor(tint);

			sprite.draw(view.getProjectionSpaceSpriteBatch());
		}
	}

	
	/**
	 * Updates the saved vertex data with the engine default configuration (category, sprite id and sprite value).
	 */
	public void updateSpriteCache(){
		if (getSpriteId() != 0) {
			AtlasRegion texture = AbstractGameObject.getSprite(getSpriteCategory(), getSpriteId(), getSpriteValue());
			if (texture == null) {
				Gdx.app.error("ago", "could not init sprite:" + getSpriteCategory() + "," + getSpriteId() + "," + getSpriteValue());
			} else {
				sprite = new GameSpaceSprite(texture);
			}
		}
	}

	

	//getter & setter
	/**
	 * the id of the sprite using for rendering.<br>
	 * By default is the same as the block id but in some cases some
	 * objects share one sprite so they can have the same id.
	 *
	 * @return in range [0;{@link RenderCell#OBJECTTYPESNUM}].
	 */
	public abstract byte getSpriteId();


	/**
     * Get the value. It is like a sub-id and can identify the status.
     * @return in range [0;{@link RenderCell#VALUESNUM}]. Is -1 if about to destroyed.
     */
	public abstract byte getSpriteValue();

	/**
	 * Returns the name of the object
	 *
	 * @return the name of the object
	 */
	public abstract String getName();

	/**
	 * Returns the rotation of the object.
	 *
	 * @return in degrees
	 */
	public float getRotation() {
		return rotation;
	}

	/**
	 * Returns the scale factor of the object.
	 *
	 * @return 1 is no scaling
	 */
	public float getScaling() {
		return scaling;
	}

	/**
	 * Returns true, when set as hidden. Hidden objects are not rendered even
	 * when they are clipped ("clipped" by the meaning of the raytracing).
	 *
	 * @return if the object is invisible
	 */
	public boolean isHidden() {
		return hidden;
	}

	/**
	 * Hides an object. It won't be rendered.
	 *
	 * @param hidden
	 */
	public void setHidden(boolean hidden) {
		this.hidden = hidden;
	}

	/**
	 *
	 * @param rotation set the rotation in degrees.
	 */
	public void setRotation(float rotation) {
		this.rotation = rotation;
	}

	/**
	 * Absolute scaling factor.
	 * @param scaling 1 no scaling, &gt; bigger, &lt; 1 smaller
	 */
	public void setScaling(float scaling) {
		this.scaling = scaling;
	}

	/**
	 * Give the object a tint. The default brightness is RGBA 0x808080FF so you
	 * can make it brighter and darker by modifying R, G and B.
	 *
	 * @param color refence is kept
	 */
	public void setColor(Color color) {
		if (color != null) {
			this.tint = color;
		}
	}

	/**
	 * get the tint of the object. The default brightness is RGBA 0x808080FF so
	 * you can make it brighter and darker by modifying R, G and B.
	 *
	 * @return not copy safe, not null
	 */
	public Color getColor() {
		return tint;
	}

	/**
	 *
	 * @return the sprite used for rendering
	 */
	public GameSpaceSprite getSprite() {
		return sprite;
	}

	/**
	 * Check if it is marked in this frame. Used for depth sorting.
	 * @param id camera id
	 * @return 
	 * @see com.bombinggames.wurfelengine.core.sorting.TopologicalSort#visit(RenderCell) 
	 */
	public final boolean isMarkedDS(final int id) {
		return ((marked>>id)&1) == ((AbstractGameObject.currentMarkedFlag >> id) & 1);
	}

	/**
	 * Marks as visited in the depth sorting algorithm.
	 * @param id camera id
	 * @see com.bombinggames.wurfelengine.core.sorting.TopologicalSort#visit(RenderCell) 
	 */
	public void markAsVisitedDS(final int id) {
		marked ^= (-((AbstractGameObject.currentMarkedFlag >> id) & 1) ^ marked) & (1 << id);
	}

	/**
	 * Gives information if object should be rendered.
	 *
	 * @param camera
	 * @return
	 */
	public boolean shouldBeRendered(Camera camera) {
		return true;
	}


}
