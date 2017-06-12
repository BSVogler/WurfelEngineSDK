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
package com.bombinggames.wurfelengine.core;

import com.badlogic.gdx.Gdx;
import com.badlogic.gdx.graphics.Color;
import com.badlogic.gdx.graphics.g2d.TextureRegion;
import com.badlogic.gdx.graphics.glutils.FrameBuffer;
import com.badlogic.gdx.graphics.glutils.HdpiUtils;
import com.badlogic.gdx.graphics.glutils.ShaderProgram;
import com.badlogic.gdx.graphics.glutils.ShapeRenderer;
import com.badlogic.gdx.math.Matrix4;
import com.badlogic.gdx.math.Vector2;
import com.badlogic.gdx.math.Vector3;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractEntity;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import com.bombinggames.wurfelengine.core.gameobjects.Renderable;
import com.bombinggames.wurfelengine.core.map.Chunk;
import com.bombinggames.wurfelengine.core.map.Map;
import com.bombinggames.wurfelengine.core.map.Point;
import com.bombinggames.wurfelengine.core.map.Position;
import com.bombinggames.wurfelengine.core.map.rendering.GameSpaceSprite;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import com.bombinggames.wurfelengine.core.sorting.AbstractSorter;
import com.bombinggames.wurfelengine.core.sorting.DepthValueSort;
import com.bombinggames.wurfelengine.core.sorting.NoSort;
import com.bombinggames.wurfelengine.core.sorting.TopologicalSort;
import java.util.LinkedList;

/**
 * Creates a virtual camera wich displays the game world on the viewport. A camer acan be locked to an entity.
 *
 * @author Benedikt Vogler
 */
public class Camera{

	/**
	 * the position of the camera in view space. Y-up. Read only field.
	 */
	private final Vector2 position = new Vector2();
	/**
	 * the unit length up vector of the camera
	 */
	private final Vector3 up = new Vector3(0, 1, 0);

	/**
	 * the projection matrix
	 */
	private final Matrix4 projection = new Matrix4();
	/**
	 * the viewMat matrix *
	 */
	private final Matrix4 viewMat = new Matrix4();
	/**
	 * the combined projection and viewMat matrix
	 */
	private final Matrix4 combined = new Matrix4();

	/**
	 * the viewport size
	 */
	private int screenWidth, screenHeight;

	/**
	 * the position on the screen (viewportWidth/Height ist the affiliated).
	 * Origin top left.
	 */
	private int screenPosX, screenPosY;

	/*
	default is 1, higher is closer
	*/	
	private float zoom = 1;

	private AbstractEntity focusEntity;

	private boolean fullWindow = false;

	private float shakeAmplitude;
	private float shakeTime;

	private GameView gameView;
	/**
	 * game pixels after projection into view space
	 */
	private int widthView;
	private int heightAfterProj;
	private int widthAfterProj;
	private int centerChunkX;
	private int centerChunkY;
	/**
	 * true if camera is currently rendering
	 */
	private boolean active = false;
	private LinkedList<AbstractGameObject> depthlist = new LinkedList<>();
	/**
	 * amount of objects to be rendered, used as an index during filling
	 */
	private int objectsToBeRendered = 0;
	private final Point center = new Point(0, 0, 0);

	/**
	 * The radius which is used for loading the chunks around the center. May be reduced after the first time to a smaller value.
	 */
	private int loadingRadius = 10;
	/**
	 * identifies the camera
	 */
	private int id;
	private int sampleNum;
	private FrameBuffer fbo;
	private TextureRegion fboRegion;
	private ShaderProgram postprocessshader;
	private AbstractSorter sorter;
	
	private int lastCenterX, lastCenterY;

	/**
	 * Updates the needed chunks after recaclucating the center chunk of the
	 * camera. It is set via an absolute value.
	 */
	private void initFocus() {
		centerChunkX = (int) Math.floor(position.x / Chunk.getViewWidth());
		centerChunkY = (int) Math.floor(-position.y / Chunk.getViewDepth());
		if (WE.getCVars().getValueB("mapUseChunks")) {
			checkNeededChunks();
		}
	}
	
	private void init(final GameView view, final int x, final int y, final int width, final int height){
		gameView = view;
		screenPosX = x;
		screenPosY = y;
		screenWidth = width;
		screenHeight = height;
		widthView = WE.getCVars().getValueI("renderResolutionWidth");
		setZoom(1);
		loadShader();
	}

	/**
	 * Creates a fullscale camera pointing at the middle of the map.
	 *
	 * @param view
	 */
	public Camera(final GameView view) {
		init(view, 0, 0, Gdx.graphics.getBackBufferWidth(), Gdx.graphics.getBackBufferHeight());

		Point center = Controller.getMap().getCenter();
		position.x = center.getViewSpcX();
		position.y = center.getViewSpcY();
		fullWindow = true;
		initFocus();
	}
	
	/**
	 * Creates a camera pointing at the middle of the map.
	 *
	 * @param x the position in the application window (viewport position).
	 * Origin top left
	 * @param y the position in the application window (viewport position).
	 * Origin top left
	 * @param width The width of the image (screen size) the camera creates on
	 * the application window (viewport)
	 * @param height The height of the image (screen size) the camera creates on
	 * the application window (viewport)
	 * @param view
	 */
	public Camera(final GameView view, final int x, final int y, final int width, final int height) {
		init(view, x, y, width, height);

		Point center = Controller.getMap().getCenter();
		position.x = center.getViewSpcX();
		position.y = center.getViewSpcY();
		initFocus();
		loadShader();
	}

	/**
	 * Create a camera focusin a specific coordinate. It can later be changed
	 * with <i>focusCoordinates()</i>. Screen size does refer to the output of
	 * the camera not the real size on the display.
	 *
	 * @param center the point where the camera focuses
	 * @param x the position in the application window (viewport position).
	 * Origin top left
	 * @param y the position in the application window (viewport position).
	 * Origin top left
	 * @param width The width of the image (screen size) the camera creates on
	 * the application window (viewport)
	 * @param height The height of the image (screen size) the camera creates on
	 * the application window (viewport)
	 * @param view
	 */
	public Camera(final GameView view, final int x, final int y, final int width, final int height, final Point center) {
		init(view, x, y, width, height);
		position.x = center.getViewSpcX();
		position.y = center.getViewSpcY();
		initFocus();
	}

	/**
	 * Creates a camera focusing an entity. The values are sceen-size and do
	 * refer to the output of the camera not the real display size.
	 *
	 * @param focusentity the entity wich the camera focuses and follows
	 * @param x the position in the application window (viewport position).
	 * Origin top left
	 * @param y the position in the application window (viewport position).
	 * Origin top left
	 * @param width The width of the image (screen size) the camera creates on
	 * the application window (viewport)
	 * @param height The height of the image (screen size) the camera creates on
	 * the application window (viewport)
	 * @param view
	 */
	public Camera(final GameView view, final int x, final int y, final int width, final int height, final AbstractEntity focusentity) {
		init(view, x, y, width, height);
		if (focusentity == null) {
			throw new NullPointerException("Parameter 'focusentity' is null");
		}
		WE.getConsole().add("Creating new camera which is focusing an entity: " + focusentity.getName());
		this.focusEntity = focusentity;
		if (!focusentity.hasPosition()) {
			throw new NullPointerException(focusentity.getName() + " is not spawned yet");
		}
		position.x = focusEntity.getPosition().getViewSpcX();
		position.y = (int) (focusEntity.getPosition().getViewSpcY()
						+ focusEntity.getDimensionZ() * RenderCell.PROJECTIONFACTORZ/2);//have middle of object in center
		initFocus();
	}
	
	/**
	 * 
	 */
	public void loadShader(){
//		try {
//			ShaderProgram newshader = WE.loadShader(true, WE.getWorkingDirectory().getAbsolutePath()+"/postprocess.fs", null);
//			postprocessshader = newshader;
//	} catch (Exception ex){
//			WE.getConsole().add(ex.getLocalizedMessage());
//			//could not load initial shader
//			if (postprocessshader == null){
//				Logger.getLogger(GameView.class.getName()).log(Level.SEVERE, null, ex);
//		
//			}
//		}
	}
	
	/**
	 * Updates the camera.
	 *
	 * @param dt
	 */
	public final void update(float dt) {
		if (active) {
			if (focusEntity != null && focusEntity.hasPosition()) {
				//update camera's position according to focusEntity
				Vector2 newPos = new Vector2(
					focusEntity.getPosition().getViewSpcX(),
					(int) (focusEntity.getPosition().getViewSpcY()
						+ focusEntity.getDimensionZ() * RenderCell.PROJECTIONFACTORZ/2)//have middle of object in center
				);

				//only follow if outside leap radius
				if (position.dst(newPos) > WE.getCVars().getValueI("CameraLeapRadius")) {
					Vector2 diff = position.cpy().sub(newPos);
					diff.nor().scl(WE.getCVars().getValueI("CameraLeapRadius"));
					position.x = newPos.x;
					position.y = newPos.y;
					position.add(diff);
				}
			}
			
			//aplly screen shake
			if (shakeTime > 0) {
				shakeTime -= dt;
				position.x += (float) (Math.random() * shakeAmplitude*dt % shakeAmplitude)-shakeAmplitude*0.5;
				position.y += (float) (Math.random() * shakeAmplitude*dt % shakeAmplitude)-shakeAmplitude*0.5;
			}


			//orthographic camera
			projection.setToOrtho(
				-getWorldWidthViewport()/ 2,
				getWorldWidthViewport() / 2,
				getWorldHeightViewport() / 2,
				-getWorldHeightViewport() / 2,
				-1000,
				10200
			);
			
//			projection.setToProjection(
//				100,
//				1010,
//				70,
//				-16/9f
//			);


			//set up projection matrices
			combined.set(projection);
			
//			if (false){
//				//move camera to the position
//				viewMat.setToLookAt(
//					new Vector3(position, 1),
//					new Vector3(position, -1),
//					new Vector3(-1,1,0)
//				);
//
//				Matrix4.mul(combined.val, viewMat.val);
//
//
//				//wurfel engine viewport matrix
//				//there is some scaling in M11, keep it
//				combined.val[Matrix4.M12] = combined.val[Matrix4.M11]*RenderCell.PROJECTIONFACTORZ;
//				combined.val[Matrix4.M11] *= -0.5f;
//
//				//by default z is negative?
//				combined.val[Matrix4.M22] = 0.0f; // project to z to 0
//				combined.val[Matrix4.M23] = 1f; // translate to z=1,
//			} else {
				//move camera to the position
				viewMat.setToLookAt(
					new Vector3(position.x,-position.y, 1),
					new Vector3(position.x,-position.y, -1),
					new Vector3(0,-1,0)
				);

				viewMat.rotate(1.0f, 0.00f, 0.0f, 60);

				Matrix4.mul(combined.val, viewMat.val);	
			//}
			
			//gameView.getGameSpaceSpriteBatch().setTransformMatrix(new Matrix4().idt());
			gameView.getGameSpaceSpriteBatch().setProjectionMatrix(combined);
			
			//recalculate the center position
			updateCenter();

			if (sorter == null) {
				//should react to an onchange event of cvar
				switch (WE.getCVars().getValueI("depthSorter")) {
					case 0:
						sorter = new NoSort(this);
						break;
					case 1:
						sorter = new TopologicalSort(this);
						break;
					case 2:
						sorter = new DepthValueSort(this);
						break;
				}
			}
		}
	}

	/**
	 * Check if center has to be moved and if chunks must be loaded or unloaded
	 * performs according actions.<br>
	 * 
	 */
	public void updateCenter() {
		//check for chunk movement
		int oldX = centerChunkX;
		int oldY = centerChunkY;

		//check if chunkswitch left
		if (
			getVisibleLeftBorder()
			<
			(centerChunkX-1)*Chunk.getBlocksX()
			//&& centerChunkX-1==//calculated xIndex -1
			) {
			centerChunkX--;
		}

		if (
			getVisibleRightBorder()
			>=
			(centerChunkX+2)*Chunk.getBlocksX()
			//&& centerChunkX-1==//calculated xIndex -1
			) {
			centerChunkX++;
		}

		int dxMovement = getCenter().getChunkX()-oldX;
		if (dxMovement*dxMovement > 1){
			//above relative move does not work. use absolute position of center
			centerChunkX = getCenter().getChunkX();
		}
		
		//the following commented lines were working once and is still a preferable way to do this algo because it avoid spots wher small movements causes ofen recalcucating of HSD. At the moment is absolute calculated. The commented code is relative based.
		
//		if (
//		getVisibleBackBorder()
//		<
//		chunkMap.getChunkContaining(centerChunkX, centerChunkY-1).getTopLeftCoordinate().getX()
//		//&& centerChunkX-1==//calculated xIndex -1
//		) {
//		centerChunkY--;
//		}
//		//check in viewMat space
//		if (
//		position.y- getHeightInProjSpc()/2
//		<
//		map.getBlocksZ()*RenderCell.VIEW_HEIGHT
//		-RenderCell.VIEW_DEPTH2*(
//		chunkMap.getChunkContaining(centerChunkX, centerChunkY+1).getTopLeftCoordinate().getY()+Chunk.getBlocksY()//bottom coordinate
//		)
//		//&& centerChunkX-1==//calculated xIndex -1
//		) {
//		centerChunkY++;
//		}

		//this line is needed because the above does not work, calcualtes absolute position
		centerChunkY = (int) Math.floor(-position.y / Chunk.getViewDepth());

		//check if center changed
		if (lastCenterX != centerChunkX
			|| lastCenterY != centerChunkY) {
			//update the last center
			lastCenterX = centerChunkX;
			lastCenterY = centerChunkY;
			//rebuild
			RenderCell.rebuildCoverList();
		}

		if (sorter instanceof TopologicalSort) {
			((TopologicalSort) sorter).rebuildTopLevelCache();
		}

		checkNeededChunks();
	}

	/**
	 * checks which chunks must be loaded around the center
	 */
	private void checkNeededChunks() {
		//check every chunk
		if (centerChunkX == 0 && centerChunkY == 0 || WE.getCVars().getValueB("mapChunkSwitch")) {
			for (int x = -loadingRadius; x <= loadingRadius; x++) {
				int lRad = loadingRadius/2;
				if (lRad <= 2) {
					lRad = 2;
				}
				for (int y = -lRad; y <= lRad; y++) {
					checkChunk(centerChunkX + x, centerChunkY + y);
				}
			}
			//after the first time reduce
			if (loadingRadius > 2) {
				loadingRadius = 2;
			}
		}
	}

	/**
	 * Checks if chunk must be loaded or deleted.
	 *
	 * @param x
	 * @param y
	 */
	private void checkChunk(int x, int y) {
		Map chunkMap = Controller.getMap();
		if (chunkMap.getChunk(x, y) == null) {
			chunkMap.loadChunk(x, y);//load missing chunks
		}
	}

	/**
	 * Renders the viewport
	 *
	 * @param view
	 * @param camera
	 */
	public void render(final GameView view, final Camera camera) {
		if (active && Controller.getMap() != null) { //render only if map exists
			
//			//render offscreen
//			screenWidth=1024;
//			screenHeight=1024;
//			if (fbo == null) {
//				fbo = new FrameBuffer(Format.RGBA8888, screenWidth, screenHeight, true);
//			}
//			if (fboRegion == null) {
//				fboRegion = new TextureRegion(fbo.getColorBufferTexture(), 0, 0,
//					screenWidth, screenHeight);
//				fboRegion.flip(false, true);
//			}
//			Gdx.gl.glClearColor(0f, 1f, 0f, 0f);
//			Gdx.gl.glClear(GL20.GL_COLOR_BUFFER_BIT | GL20.GL_DEPTH_BUFFER_BIT);
//			Gdx.gl20.glViewport(0, 0, fbo.getWidth(), fbo.getHeight());
//			fbo.begin();
			
		//view.getGameSpaceSpriteBatch().setTransformMatrix(new Matrix4().idt());
		//	view.getGameSpaceSpriteBatch().setProjectionMatrix(combined);//game space
			
			ShaderProgram shader = view.getShader();
			if (shader==null) {
				Gdx.app.error("Camera", "no shader found");
				return;
			}
			
			view.getGameSpaceSpriteBatch().setShader(shader);
			//set up the viewport, yIndex-up
			HdpiUtils.glViewport(screenPosX,
				Gdx.graphics.getHeight() - getHeightScreenSpc() - screenPosY,
				getWidthScreenSpc(),
				getHeightScreenSpc()
			);

			//Gdx.gl20.glBlendFunc(GL_SRC_ALPHA, GL20.GL_CONSTANT_COLOR);

			//settings for this frame
			RenderCell.setStaticShade(WE.getCVars().getValueB("enableAutoShade"));
			GameSpaceSprite.setAO(WE.getCVars().getValueF("ambientOcclusion"));
			
			view.getGameSpaceSpriteBatch().begin();
			//upload uniforms
			shader.setUniformf("cameraPos",getCenter());
			shader.setUniformf("fogColor",
				WE.getCVars().getValueF("fogR"),
				WE.getCVars().getValueF("fogG"),
				WE.getCVars().getValueF("fogB")
			);
			if (focusEntity!=null)
				shader.setUniformf("playerpos",focusEntity.getPoint());
			//send a Vector4f to GLSL
			if (WE.getCVars().getValueB("enablelightengine")) {
				shader.setUniformf(
					"sunNormal",
					Controller.getLightEngine().getSun(getCenter()).getNormal()
				);
				shader.setUniformf(
					"sunColor",
					Controller.getLightEngine().getSun(getCenter()).getLight()
				);
				
				Vector3 moonNormal;
				Color moonColor;
				Color ambientColor;
				if (Controller.getLightEngine().getMoon(getCenter()) == null) {
					moonNormal = new Vector3();
					moonColor = new Color();
					ambientColor = new Color();
				} else {
					moonNormal = Controller.getLightEngine().getMoon(getCenter()).getNormal();
					moonColor = Controller.getLightEngine().getMoon(getCenter()).getLight();
					ambientColor = Controller.getLightEngine().getAmbient(getCenter());
				}
				shader.setUniformf("moonNormal", moonNormal);
				shader.setUniformf("moonColor", moonColor);
				shader.setUniformf("ambientColor", ambientColor);
			}

			//bind diffuse color to texture unit 0
			//important that we specify 0 otherwise we'll still be bound to glActiveTexture(GL_TEXTURE1)
			AbstractGameObject.getTextureDiffuse().bind(0);
			
			//bind normal map to texture unit 1
			if (WE.getCVars().getValueB("LEnormalMapRendering")) {
				AbstractGameObject.getTextureNormal().bind(1);
			}

			//render vom bottom to top
			sorter.renderSorted();
			view.getGameSpaceSpriteBatch().end();

			//debug rendering
			if (WE.getCVars().getValueB("DevDebugRendering")) {
				drawDebug(view, camera);
			}
			//to render offscreen onscreen
//			fbo.end();
//			
//			OrthographicCamera cam = new OrthographicCamera(Gdx.graphics.getWidth(),
//				Gdx.graphics.getHeight());
//			cam.setToOrtho(false, Gdx.graphics.getWidth(), Gdx.graphics.getHeight());
//			view.getProjectionSpaceSpriteBatch().setProjectionMatrix(cam.combined);
//			view.getProjectionSpaceSpriteBatch().setShader(postprocessshader);
//			//fboRegion.getTexture().bind();
//			Gdx.gl.glClearColor(0f, 0f, 0f, 0f);
//			Gdx.gl.glClear(GL20.GL_COLOR_BUFFER_BIT | GL20.GL_DEPTH_BUFFER_BIT);
//			view.getProjectionSpaceSpriteBatch().begin();
//			view.getProjectionSpaceSpriteBatch().draw(fboRegion, 0, 0,Gdx.graphics.getWidth(),Gdx.graphics.getHeight());
//			view.getProjectionSpaceSpriteBatch().end();
		}
	}

	

	/**
	 * checks if the projected position is inside the viewMat Frustum
	 *
	 * @param pos
	 * @return
	 */
	public boolean inViewFrustum(Position pos){
		int vspY = pos.getViewSpcY();
		if (!(
				(position.y + (heightAfterProj>>1))//fast division by two
				>
				(vspY - (RenderCell.VIEW_HEIGHT<<1))//bottom of sprite
			&&
				(vspY + RenderCell.VIEW_HEIGHT2 + RenderCell.VIEW_DEPTH)//top of sprite
				>
				position.y - (heightAfterProj>>1))//fast division by two
//fast division by two
		)
			return false;
		int dist = (int) (pos.getViewSpcX()-position.x); //left side of sprite
		//left and right check in one clause by using distance via squaring
		return dist * dist < ( (widthAfterProj >> 1) + RenderCell.VIEW_WIDTH2) * ((widthAfterProj >> 1) + RenderCell.VIEW_WIDTH2);
	}

	

	/**
	 * Returns the left border of the actual visible area.
	 *
	 * @return left x position in view space
	 */
	public float getVisibleLeftBorderVS() {
		return (position.x - widthAfterProj*0.5f)- RenderCell.VIEW_WIDTH2;
	}

	/**
	 * Returns the left border of the actual visible area.
	 *
	 * @return the left (X) border coordinate
	 */
	public int getVisibleLeftBorder() {
		return (int) ((position.x - widthAfterProj*0.5) / RenderCell.VIEW_WIDTH - 1);
	}

	/**
	 * Returns the right seight border of the camera covered area currently
	 * visible.
	 *
	 * @return measured in grid-coordinates
	 */
	public int getVisibleRightBorder() {
		return (int) ((position.x + widthAfterProj*0.5) / RenderCell.VIEW_WIDTH + 1);
	}
	
	/**
	 * Returns the right seight border of the camera covered area currently
	 * visible.
	 *
	 * @return measured in grid-coordinates
	 */
	public float getVisibleRightBorderVS() {
		return position.x + widthAfterProj*0.5f + RenderCell.VIEW_WIDTH2;
	}

	/**
	 * Returns the top seight border of the camera covered groundBlock
	 *
	 * @return measured in grid-coordinates
	 */
	public int getVisibleBackBorder() {
		//TODO verify
		return (int) ((position.y + heightAfterProj * 0.5)//camera top border
			/ -RenderCell.VIEW_DEPTH2//back to game space
			);
	}

	/**
	 * Returns the bottom seight border y-coordinate of the lowest cell
	 *
	 * @return measured in grid-coordinates
	 * @see #getVisibleFrontBorderHigh()
	 */
	public int getVisibleFrontBorderLow() {
		return (int) ((position.y - heightAfterProj * 0.5) //bottom camera border
			/ -RenderCell.VIEW_DEPTH2 //back to game coordinates
			);
	}

	/**
	 * Returns the bottom seight border y-coordinate of the highest cell
	 *
	 * @return measured in grid-coordinates
	 * @see #getVisibleFrontBorderLow()
	 */
	public int getVisibleFrontBorderHigh() {
		return (int) ((position.y - heightAfterProj * 0.5) //bottom camera border
			/ -RenderCell.VIEW_DEPTH2 //back to game coordinates
			+ Chunk.getBlocksY() * 3 * RenderCell.VIEW_HEIGHT / RenderCell.VIEW_DEPTH2 //todo verify, try to add z component
			);
	}

	/**
	 * The Camera Position in the game world.
	 *
	 * @return game in pixels
	 */
	public float getViewSpaceX() {
		return getCenter().getViewSpcX();
	}

	/**
	 * The Camera's center position in the game world. viewMat space. yIndex up
	 *
	 * @return in camera position game space
	 */
	public float getViewSpaceY() {
		return getCenter().getViewSpcY();
	}

	/**
	 * Set the zoom factor.
	 *
	 * @param zoom 1 is default
	 */
	public void setZoom(float zoom) {
		this.zoom = zoom;
		widthAfterProj = (int) (widthView / zoom);//update cache
		heightAfterProj = (int) (screenHeight / (getProjScaling()*zoom));
	}

	/**
	 * the width of the internal render resolution
	 *
	 * @param resolution
	 */
	public void setInternalRenderResolution(int resolution) {
		widthView = resolution;
	}

	/**
	 * Returns the zoomfactor.
	 *
	 * @return zoomfactor applied on the game world
	 */
	public float getZoom() {
		return zoom;
	}

	/**
	 * Returns a scaling factor calculated by the width to achieve the same
	 * viewport size with every resolution. If displayed twice as big as render resolution has factor 2.
	 *
	 * @return a scaling factor applied on the projection
	 */
	public float getProjScaling() {
		return screenWidth / (float) widthView;
	}
	
	/**
	 * The amount of game pixels which are visible in X direction after
	 * the zoom has been applied. For screen pixels use
	 * {@link #getWidthScreenSpc()}.
	 *
	 * @return in projection space
	 */
	public final int getWorldWidthViewport() {
		return widthAfterProj;
	}

	/**
	 * The amount of game pixel which are visible in Y direction after the zoom
	 * has been applied. For screen pixels use {@link #getHeightScreenSpc() }.
	 *
	 * @return in projection space
	 */
	public final int getWorldHeightViewport() {
		return heightAfterProj;
	}
	
	/**
	 * Returns the position of the cameras output (on the screen)
	 *
	 * @return in projection pixels
	 */
	public int getScreenPosX() {
		return screenPosX;
	}

	/**
	 * Returns the position of the camera (on the screen)
	 *
	 * @return yIndex-down
	 */
	public int getScreenPosY() {
		return screenPosY;
	}

	/**
	 * Returns the height of the camera output.
	 *
	 * @return the value before scaling
	 */
	public int getHeightScreenSpc() {
		return screenHeight;
	}

	/**
	 * Returns the width of the camera output.
	 *
	 * @return the value before scaling
	 */
	public int getWidthScreenSpc() {
		return screenWidth;
	}

	/**
	 * Does the cameras output cover the whole screen?
	 *
	 * @return
	 */
	public boolean isFullWindow() {
		return fullWindow;
	}

	/**
	 * Set to true if the camera's output should cover the whole window
	 *
	 * @param fullWindow
	 */
	public void setFullWindow(boolean fullWindow) {
		this.fullWindow = fullWindow;
		if (fullWindow){
			this.screenWidth = Gdx.graphics.getWidth();
			this.screenHeight = Gdx.graphics.getHeight();
			this.screenPosX = 0;
			this.screenPosY = 0;
		}
	}

	/**
	 * Should be called when resized
	 *
	 * @param width width of window
	 * @param height height of window
	 */
	public void resize(int width, int height) {
		if (fullWindow) {
			this.screenWidth = width;
			this.screenHeight = height;
			this.screenPosX = 0;
			this.screenPosY = 0;
		}
	}

	/**
	 * updates the screen size
	 *
	 * @param width
	 * @param height
	 */
	public void setScreenSize(int width, int height) {
		if (width < Gdx.graphics.getWidth() || height < Gdx.graphics.getHeight()) {
			fullWindow = false;
		}
		this.screenWidth = width;
		this.screenHeight = height;
	}

	/**
	 * Move x and y coordinate
	 *
	 * @param x in game space
	 * @param y in game space
	 */
	public void move(int x, int y) {
		if (focusEntity != null) {
			focusEntity.getPosition().add(x, y, 0);
		} else {
			position.x += x;
			position.y -= y/2;
		}
		updateCenter();
	}

	/**
	 * shakes the screen
	 *
	 * @param amplitude
	 * @param time game time
	 */
	public void shake(float amplitude, float time) {
		shakeAmplitude = amplitude;
		shakeTime = time;
	}

	/**
	 * Returns the focuspoint. Approximated because is stored in view space and backtransformation is a line.
	 *
	 * @return in game space, copy safe
	 */
	public Point getCenter() {
		return (Point) center.set(
			position.x,
			-(position.y-RenderCell.VIEW_HEIGHT2*Chunk.getBlocksZ()) / RenderCell.PROJECTIONFACTORY,
			RenderCell.GAME_EDGELENGTH2*Chunk.getBlocksZ()
		);//view to game
	}
	
	/**
	 * Set the cameras center to a point. If the camera is locked to a an entity this lock will be removed.
	 * @param point game space. z gets ignored
	 */
	public void setCenter(Point point){
		focusEntity = null;
		position.x = point.getViewSpcX();
		position.y = point.getViewSpcY();//game to view space transformation
	}

	/**
	 *
	 * @param focusEntity
	 */
	public void setFocusEntity(AbstractEntity focusEntity) {
		if (this.focusEntity != focusEntity) {
			this.focusEntity = focusEntity;
			position.set(focusEntity.getPosition().getViewSpcX(),
				(int) (focusEntity.getPosition().getViewSpcY()
					+ focusEntity.getDimensionZ() * RenderCell.PROJECTIONFACTORZ/2)//have middle of object in center
			);
		}
	}
	
	/**
	 * enable or disable the camera
	 *
	 * @param active
	 */
	public void setActive(boolean active) {
		//turning on
		if (!this.active && active) {
			if (WE.getCVars().getValueB("mapUseChunks")) {
				checkNeededChunks();
			}
		}

		this.active = active;
	}

	/**
	 * 
	 * @param view
	 * @param camera 
	 */
	private void drawDebug(GameView view, Camera camera) {
		//outline 3x3 chunks
		ShapeRenderer sh = view.getShapeRenderer();
		sh.setProjectionMatrix(combined);//draw in game space
		sh.setColor(Color.RED.cpy());
		sh.begin(ShapeRenderer.ShapeType.Line);
		sh.rect(-Chunk.getGameWidth(),//one chunk to the left
			-Chunk.getGameDepth(),//two chunks down
			Chunk.getGameWidth()*3,
			Chunk.getGameDepth()*3 / 2
		);
		sh.line(-Chunk.getGameWidth(),
			-Chunk.getGameDepth() / 2,
			-Chunk.getGameWidth() + Chunk.getGameWidth()*3,
			-Chunk.getGameDepth() / 2
		);
		view.getShapeRenderer().line(-Chunk.getGameWidth(),
			0,
			-Chunk.getGameWidth() + Chunk.getGameWidth()*3,
			0
		);
		sh.line(
			0,
			Chunk.getGameDepth() / 2,
			0,
			-Chunk.getGameDepth()
		);
		sh.line(
			Chunk.getGameWidth(),
			Chunk.getGameDepth() / 2,
			Chunk.getGameWidth(),
			-Chunk.getGameDepth()
		);
		sh.end();
		
		view.resetProjectionMatrix();
		sh.begin(ShapeRenderer.ShapeType.Filled);
		//render vom bottom to top
//		for (AbstractEntity ent : Controller.getMap().getEntities()) {
//			sh.setColor(Color.GREEN);
//			//life bar
//			sh.rect(
//				ent.getPoint().getProjectionSpaceX(view, camera),
//				ent.getPoint().getProjectionSpaceY(view, camera) + RenderCell.VIEW_HEIGHT*ent.getScaling(),
//				ent.getHealth() / 100.0f * RenderCell.VIEW_WIDTH2*ent.getScaling(),
//				5
//			);
//		}

		Color linecolor =new Color(0, 1, 1, 1); 
		sh.setColor(linecolor);
		Renderable last = null;
		sorter.createDepthList(depthlist);
		
		for (Renderable current : depthlist) {
			if (last==null){
				last = current;
				continue;
			}
			linecolor.add(1/(float) depthlist.size(), -1/(float) depthlist.size(), 0, 0);
			sh.setColor(linecolor);
			sh.line(last.getPosition().getProjectionSpaceX(view, camera), last.getPosition().getProjectionSpaceY(view, camera), current.getPosition().getProjectionSpaceX(view, camera), current.getPosition().getProjectionSpaceY(view, camera));
			last = current;
		}
		sh.end();
		
	}

	public GameView getGameView() {
		return gameView;
	}
	

	/**
	 *
	 * @return
	 */
	public int getCenterChunkX() {
		return centerChunkX;
	}

	/**
	 *
	 * @return
	 */
	public int getCenterChunkY() {
		return centerChunkY;
	}

	/**
	 *
	 * @return
	 */
	public boolean isEnabled() {
		return active;
	}

	/**
	 *
	 * @param id
	 */
	public void setId(int id){
		this.id = id;
	}
	
	public int getId() {
		return id;
	}

	/**
	 * may be overwritten
	 */
	void dispose() {
	}

}
