/*
 * Copyright 2015 Benedikt Vogler.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * * If this software is used for a game the official „Wurfel Engine“ logo or its name must be
 *   visible in an intro screen or main menu.
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
import com.badlogic.gdx.Input.Keys;
import com.badlogic.gdx.ai.msg.MessageManager;
import com.badlogic.gdx.graphics.Color;
import com.badlogic.gdx.graphics.GL20;
import com.badlogic.gdx.graphics.OrthographicCamera;
import com.badlogic.gdx.graphics.Pixmap;
import com.badlogic.gdx.graphics.g2d.SpriteBatch;
import com.badlogic.gdx.graphics.glutils.FrameBuffer;
import com.badlogic.gdx.graphics.glutils.HdpiUtils;
import com.badlogic.gdx.graphics.glutils.ShaderProgram;
import com.badlogic.gdx.graphics.glutils.ShapeRenderer;
import com.badlogic.gdx.math.Vector3;
import com.badlogic.gdx.scenes.scene2d.Stage;
import com.badlogic.gdx.utils.viewport.StretchViewport;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import com.bombinggames.wurfelengine.core.map.Chunk;
import com.bombinggames.wurfelengine.core.map.Intersection;
import com.bombinggames.wurfelengine.core.map.LoadMenu;
import com.bombinggames.wurfelengine.core.map.Point;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import com.bombinggames.wurfelengine.core.map.rendering.RenderStorage;
import com.bombinggames.wurfelengine.core.map.rendering.SpriteBatchWithZAxis;
import java.io.FileNotFoundException;
import java.util.ArrayList;
import java.util.logging.Level;
import java.util.logging.Logger;

/**
 * The GameView manages everything what should be drawn in an active game in game space. It includes two batches. One for gamespace and one for projection space.<br>
 * 
 * Code which needs a running instance should be put into the {@link #init(com.bombinggames.wurfelengine.core.Controller, com.bombinggames.wurfelengine.core.GameView) } method.
 * @author Benedikt
 */
public class GameView implements GameManager {

	/**
	 * saves the amount of active cameras
	 */
	private static int cameraIdCounter = 0;

	/**
     * Shoud be called before the object get initialized.
     * Initializes class fields.
     */
    public static void classInit(){
			//set up font
			//font = WurfelEngine.getInstance().manager.get("com/bombinggames/wurfelengine/EngineCore/arial.fnt"); //load font
			//font.scale(2);
			
			//font.scale(-0.5f);
			
			//load sprites
		try {
			RenderCell.loadSheet();
		} catch (FileNotFoundException ex) {
			Logger.getLogger(GameView.class.getName()).log(Level.SEVERE, null, ex);
		}
    }
	
	/**
	 * the cameras rendering the scene
	 */
    private final ArrayList<Camera> cameras = new ArrayList<>(6);//max 6 cameras
    
    private ShaderProgram shader;
    private ShapeRenderer shRenderer;
    
    private Controller controller;
    
	/**
	 * a camera rendering the gui and hud
	 */
    private OrthographicCamera libGDXcamera;
    private boolean keyF5isUp;
    
    /**
     * game related stage. e.g. holds hud and gui
     */
    private Stage stage;
    private final SpriteBatchWithZAxis gameSpaceSpriteBatch = new SpriteBatchWithZAxis(WE.getCVars().getValueI("MaxSprites"));
	private final SpriteBatch projectionSpaceSpriteBatch = new SpriteBatch(1000);
    
    private LoadMenu loadMenu;
    
        
    private boolean initalized;
	
	/**
	 * backup of cvar for keeping the time steady when changed in another view
	 */
	private float gameSpeed = 1f;
	
	private boolean useDefaultShader;
	
	private RenderStorage renderstorage;
	/**
	 * the sprites rendered so fat
	 */
	private int numSpritesThisFrame;
	private int depthTexture;
	private int depthTexture1;
	private FrameBuffer[] fbo;
	private ShaderProgram depthShader;
    
	/**
	 * Loades some files and set up everything when the engine is running. After this has been inactive use {@link #onEnter() }<br>
	 * This method is a an implementation of the <a href="https://de.wikipedia.org/wiki/Dependency_Injection">Setter Injection</a> pattern.
	 *
	 * @param controller The dependent data controller used for the view. Can be null but
	 * should not.
	 * @param oldView The view used before. Can be null.
	 * @see #onEnter() 
	 */
	public void init(final Controller controller, final GameView oldView) {
		Gdx.app.debug("GameView", "Initializing");
		loadShaders();

		this.controller = controller;

		//clear old stuff
		cameras.clear();

		//set up renderer
		libGDXcamera = new OrthographicCamera(Gdx.graphics.getWidth(), Gdx.graphics.getHeight());

		shRenderer = new ShapeRenderer();

		//set up stage
		stage = new Stage(
			new StretchViewport(
				Gdx.graphics.getWidth(),
				Gdx.graphics.getHeight()
			),
			projectionSpaceSpriteBatch
		);//spawn at fullscreen

		useDefaultShader();//set default shader

		renderstorage = new RenderStorage();
		MessageManager.getInstance().addListener(renderstorage, Events.mapChanged.getId());
		initalized = true;
	}
	
	
	/**
	 * Get the loaded shader program of the view.
	 *
	 * @return
	 */
	public ShaderProgram getShader() {
		return shader;
	}
	
	/**
	 * reloads the shaders
	 */
	public void loadShaders() {
		Gdx.app.debug("Shader", "loading");
		
		//try loading external shader
		String fragment = WE.getWorkingDirectory().getAbsolutePath() + "/fragment"
			+ (WE.getCVars().getValueB("LEnormalMapRendering") ? "_NM": "")
			+ ".fs";
		String vertex = WE.getWorkingDirectory().getAbsolutePath() + "/vertex.vs";

		ShaderProgram newshader = null;
		try {
			newshader = WE.loadShader(false, fragment, vertex);
			for (Camera camera : cameras) {
				camera.loadShader();
			}
		} catch (Exception ex) {
			WE.getConsole().add(ex.getLocalizedMessage());
			Logger.getLogger(GameView.class.getName()).log(Level.SEVERE, null, ex);
		}

		//could not load initial external shader, so try loading internal
		if (newshader == null) {
			fragment = "com/bombinggames/wurfelengine/core/fragment"
				+ (WE.getCVars().getValueB("LEnormalMapRendering") ? "_NM" : "")
				+ ".fs";
			vertex = "com/bombinggames/wurfelengine/core/vertex.vs";
			try {
				newshader = WE.loadShader(true, fragment, vertex);
			} catch (Exception ex) {
				WE.getConsole().add(ex.getLocalizedMessage());
				if (newshader == null) {
					Logger.getLogger(GameView.class.getName()).log(Level.SEVERE, null, ex);
				}
			}
		}

		if (newshader != null) {
			shader = newshader;
			//setup default uniforms
			shader.begin();
			//our normal map
			shader.setUniformi("u_normals", 1); //GL_TEXTURE1
			if (WE.getCVars().getValueI("depthbuffer") == 2) {
				shader.setUniformi("u_depth", 2); //GL_TEXTURE2
			}
			shader.end();	
		}
		
		if (WE.getCVars().getValueI("depthbuffer") == 2){
			try {
				String frag = WE.getWorkingDirectory().getAbsolutePath() + "/fragment_DP.fs";
				String vert = WE.getWorkingDirectory().getAbsolutePath() + "/vertex.vs";
				depthShader = WE.loadShader(false, frag, vert);
				depthShader.begin();
				//our normal map
				depthShader.setUniformi("u_normals", 1); //GL_TEXTURE1
				depthShader.setUniformi("u_depth", 2); //GL_TEXTURE2
				depthShader.end();
			} catch (Exception ex) {
				Logger.getLogger(GameView.class.getName()).log(Level.SEVERE, null, ex);
			}
		}
	}

	@Override
	public void onEnter() {
		//no code here intentionally, use enter() instead
	}

	@Override
	public final void enter() {//by using final this can not be overwritten
		Gdx.app.debug("GameView", "Entering");
		if (!isInitalized()) {
			Gdx.app.error(this.getClass().toString(), "Called method enter() before initializing.");
		}
		WE.getEngineView().addInputProcessor(stage);//the input processor must be added every time because they are only 
		
		//enable cameras
		for (Camera camera : cameras) {
			camera.setActive(true);
		}
		
		if (WE.SOUND != null) {
			WE.SOUND.setView(this);
		}

		if ((boolean) WE.getCVars().get("DevMode").getValue()) {
			WE.getEngineView().setCursor(0);
		}

		resize(Gdx.graphics.getWidth(), Gdx.graphics.getHeight());

		//restore gameSpeed
		WE.getCVars().get("timespeed").setValue(gameSpeed);
		
		onEnter();//some pattern to call onEnter() and deny overrides to enter
	}
	
	/**
	 * update before the game logic calls
	 * @param dt 
	 */
	public void preUpdate(final float dt){
		renderstorage.preUpdate(dt);
	}

    /**
     *Updates every camera and everything else which must be updated.
     * @param dt time since last update in ms.
     */
    public void update(final float dt){
		gameSpeed = WE.getCVars().getValueF("timespeed");
		
        stage.act(dt);
		        
        //update cameras
		/**
		 * problem! Write acces in view. causes 1 frame hack without hacks. Workaround by post-update method.
		 */
		//at least one active camera
		boolean cameraactive = false;
		for (Camera camera : cameras) {
			camera.update(dt);
			if (camera.isEnabled()) {
				cameraactive = true;
			}
		}
		if (cameraactive) {
			renderstorage.update(dt);
		}

		// toggle the dev menu?
		if (keyF5isUp && Gdx.input.isKeyPressed(Keys.F5)) {
			controller.getDevTools().setVisible(!controller.getDevTools().isVisible());
			keyF5isUp = false;
		}
		keyF5isUp = !Gdx.input.isKeyPressed(Keys.F5);
	}

	/**
	 *
	 * @return
	 */
	public RenderStorage getRenderStorage() {
		return renderstorage;
	}

	/**
	 *
	 * @param renderstorage
	 */
	public void setRenderStorage(RenderStorage renderstorage) {
		if (this.renderstorage != null) {
			MessageManager.getInstance().removeListener(this.renderstorage, Events.mapChanged.getId());
		}
		this.renderstorage = renderstorage;
	}
	
    /**
     * Main method which is called every time and renders everything. You must manually render the devtools e.g. in an extended render method.
     */
    public void render(){       
        //Gdx.gl10.glViewport(0, 0,Gdx.graphics.getWidth(), Gdx.graphics.getHeight());
        
        //clear screen if wished
       if (WE.getCVars().getValueB(("clearBeforeRendering"))) {
			Gdx.gl20.glClearColor(0, 0, 0, 1);//black
			if (WE.getCVars().getValueI(("depthbuffer"))>0) {
				Gdx.gl20.glEnable(GL20.GL_DEPTH_TEST);
				Gdx.gl.glClearDepthf(1f);
				Gdx.gl20.glDepthMask(true); // enable depth depthTexture writes
				Gdx.gl.glDepthRangef(0, 1);
				Gdx.gl.glDepthFunc(GL20.GL_LEQUAL);
				Gdx.gl20.glClear(GL20.GL_COLOR_BUFFER_BIT | GL20.GL_DEPTH_BUFFER_BIT);
			} else {
				Gdx.gl20.glDisable(GL20.GL_DEPTH_TEST);
				Gdx.gl20.glClear(GL20.GL_COLOR_BUFFER_BIT);
			}
		}
	   
	   int spritesStart = gameSpaceSpriteBatch.getRenderedSprites();

        //render every camera
        if (cameras.isEmpty()){
            Gdx.gl20.glClearColor(0.5f, 1, 0.5f, 1);
            Gdx.gl20.glClear(GL20.GL_COLOR_BUFFER_BIT);
            drawString("No camera set up", Gdx.graphics.getWidth()/2, Gdx.graphics.getHeight()/2, Color.BLACK.cpy());
        } else {
					
			int xres = Gdx.graphics.getBackBufferWidth();
			int yres = Gdx.graphics.getBackBufferHeight();		
			
			//if depth peeling enabled, dual pass rendering
			if (WE.getCVars().getValueI("depthbuffer") == 2) {
				ShaderProgram regularShader = shader;
				if (depthShader == null) {
					loadShaders();
				}
				shader = depthShader;
				//create new depthtexture if needed
				if (depthTexture == 0) {
					depthTexture = Gdx.gl.glGenTexture();
					//bind/upload? depth texture
					Gdx.gl.glBindTexture(GL20.GL_TEXTURE_2D, depthTexture);
					//Gdx.gl.glActiveTexture(GL20.GL_TEXTURE0 + 2);

					//specifiy last bound texture to be a depth texture
					Gdx.gl.glTexImage2D(GL20.GL_TEXTURE_2D, 0, GL20.GL_DEPTH_COMPONENT16, xres, yres, 0, GL20.GL_DEPTH_COMPONENT, GL20.GL_FLOAT, null);
					Gdx.gl.glTexParameteri(GL20.GL_TEXTURE_2D, GL20.GL_TEXTURE_MAG_FILTER, GL20.GL_NEAREST);
					Gdx.gl.glTexParameteri(GL20.GL_TEXTURE_2D, GL20.GL_TEXTURE_MIN_FILTER, GL20.GL_NEAREST); 
					Gdx.gl.glTexParameteri(GL20.GL_TEXTURE_2D, GL20.GL_TEXTURE_WRAP_S, GL20.GL_CLAMP_TO_EDGE);
					Gdx.gl.glTexParameteri(GL20.GL_TEXTURE_2D, GL20.GL_TEXTURE_WRAP_T, GL20.GL_CLAMP_TO_EDGE);
				}
				
				if (depthTexture1 == 0) {
					depthTexture1 = Gdx.gl.glGenTexture();
					//bind/upload? depth texture
					Gdx.gl.glBindTexture(GL20.GL_TEXTURE_2D, depthTexture1);
					//Gdx.gl.glActiveTexture(GL20.GL_TEXTURE0 + 2);

					
					//specifiy last bound texture to be a depth texture
					Gdx.gl.glTexImage2D(GL20.GL_TEXTURE_2D, 0, GL20.GL_DEPTH_COMPONENT16, xres, yres, 0, GL20.GL_DEPTH_COMPONENT, GL20.GL_FLOAT, null);
					Gdx.gl.glTexParameteri(GL20.GL_TEXTURE_2D, GL20.GL_TEXTURE_MAG_FILTER, GL20.GL_NEAREST);
					Gdx.gl.glTexParameteri(GL20.GL_TEXTURE_2D, GL20.GL_TEXTURE_MIN_FILTER, GL20.GL_NEAREST); 
					Gdx.gl.glTexParameteri(GL20.GL_TEXTURE_2D, GL20.GL_TEXTURE_WRAP_S, GL20.GL_CLAMP_TO_EDGE);
					Gdx.gl.glTexParameteri(GL20.GL_TEXTURE_2D, GL20.GL_TEXTURE_WRAP_T, GL20.GL_CLAMP_TO_EDGE);
				}
				
//				int renderbuffer = Gdx.gl.glGenRenderbuffer();
//				Gdx.gl.glBindRenderbuffer(GL20.GL_RENDERBUFFER, renderbuffer);
//				Gdx.gl.glRenderbufferStorage(GL20.GL_RENDERBUFFER, GL20.GL_DEPTH_COMPONENT, 1024, 768);
//				Gdx.gl.glFramebufferRenderbuffer(GL20.GL_FRAMEBUFFER, GL20.GL_DEPTH_ATTACHMENT, GL20.GL_RENDERBUFFER, renderbuffer);
				
				// Set "renderedTexture" as our colour attachement #0
				//Gdx.gl.glFramebufferTexture(GL20.GL_FRAMEBUFFER, GL20.GL_COLOR_ATTACHMENT0, renderedTexture, 0);
				//Gdx.gl.glTexImage2D(GL20.GL_TEXTURE_2D, 0,GL20.GL_RGB, 1024, 768, 0,GL20.GL_RGB, GL20.GL_UNSIGNED_BYTE, null);
				
				//render offsceen
				int numLayers = 2;
				if (fbo == null) {
					fbo = new FrameBuffer[3];
				}
				for (int i = 0; i < numLayers; i++) {
					if (fbo[0] == null) {
						fbo[0] = new FrameBuffer(Pixmap.Format.RGBA8888, xres, yres, true);
					}
					if (numLayers>1 &&fbo[1] == null) {
						fbo[1] = new FrameBuffer(Pixmap.Format.RGBA8888, xres, yres, false);
					}
					//render to fbo
					if (i == 0) {
						gameSpaceSpriteBatch.disableBlending();
						fbo[i].begin();//same as Gdx.gl.glBindFramebuffer(GL20.GL_FRAMEBUFFER, fbo[0].getFramebufferHandle());
					} else if (i == numLayers-1) {
						gameSpaceSpriteBatch.enableBlending();
					}
					//active framebuffer, attach this texture as your depth buffer from now on
					//Owrites to both frame buffers after second run
					Gdx.gl.glFramebufferTexture2D(GL20.GL_FRAMEBUFFER, GL20.GL_DEPTH_ATTACHMENT, GL20.GL_TEXTURE_2D, i % 2 == 0 ? depthTexture1 : depthTexture, 0);

					if (Gdx.gl.glCheckFramebufferStatus(GL20.GL_FRAMEBUFFER) != GL20.GL_FRAMEBUFFER_COMPLETE) {
						throw new AbstractMethodError();
					}

					//use last texture for read-only
					Gdx.gl.glActiveTexture(GL20.GL_TEXTURE0 + 2);
					Gdx.gl.glBindTexture(GL20.GL_TEXTURE_2D, i % 2 == 0 ? depthTexture : depthTexture1);
					//bind normal map to texture unit 1
					if (WE.getCVars().getValueB("LEnormalMapRendering")) {
						AbstractGameObject.getTextureNormal().bind(1);
					}
					AbstractGameObject.getTextureDiffuse().bind(0);

					//clear color of frame buffer
					if (WE.getCVars().getValueB(("clearBeforeRendering"))) {
						Gdx.gl20.glClear(GL20.GL_COLOR_BUFFER_BIT | GL20.GL_DEPTH_BUFFER_BIT);
					}else{
						Gdx.gl20.glClear(GL20.GL_DEPTH_BUFFER_BIT);
					}

					//render to obtain i-th frontmost pixel
					for (Camera camera : cameras) {
						camera.render(this);
					}
					Gdx.gl.glFramebufferTexture2D(GL20.GL_FRAMEBUFFER, 0, GL20.GL_TEXTURE_2D, 0, 0);
					if (i == 0) {
						fbo[i].end();
					}

					//clear the read only
					if (i==1){
						fbo[0].begin();
						Gdx.gl20.glClear(GL20.GL_DEPTH_BUFFER_BIT);
						fbo[0].end();
					}
//					else {
					//blend
//				Gdx.gl20.glDisable(GL20.GL_DEPTH_TEST);
//				Gdx.gl.glBlendEquation(GL20.GL_FUNC_ADD);
//                Gdx.gl.glBlendFuncSeparate(GL20.GL_DST_COLOR, GL20.GL_ONE, GL20.GL_ZERO, GL20.GL_ONE_MINUS_SRC_ALPHA);
//				projectionSpaceSpriteBatch.begin();
//				//draw flipped
////				projectionSpaceSpriteBatch.draw(fbo[1].getColorBufferTexture(), 0, yres, xres, -yres);
//				projectionSpaceSpriteBatch.draw(fbo[0].getColorBufferTexture(), 0, yres, xres, -yres);
// 				projectionSpaceSpriteBatch.end();
//					}
				}
				
				//blend
				Gdx.gl20.glDisable(GL20.GL_DEPTH_TEST);
				Gdx.gl.glBlendEquation(GL20.GL_FUNC_ADD);
                Gdx.gl.glBlendFuncSeparate(GL20.GL_DST_COLOR, GL20.GL_ONE, GL20.GL_ZERO, GL20.GL_ONE_MINUS_SRC_ALPHA);
				projectionSpaceSpriteBatch.begin();
				//draw flipped
//				projectionSpaceSpriteBatch.draw(fbo[1].getColorBufferTexture(), 0, yres, xres, -yres);
				projectionSpaceSpriteBatch.draw(fbo[0].getColorBufferTexture(), 0, yres, xres, -yres);
 				projectionSpaceSpriteBatch.end();
				
				shader = regularShader;
			} else {
				
				if (WE.getCVars().getValueI("depthbuffer") == 0) {
					gameSpaceSpriteBatch.enableBlending();
				}
				AbstractGameObject.getTextureDiffuse().bind(0);
				setShader(getShader());
				
				for (Camera camera : cameras) {
					camera.render(this);
				}
			}
		}
               
        //render HUD and GUI
		useDefaultShader();
		//to screen space?
		gameSpaceSpriteBatch.setProjectionMatrix(libGDXcamera.combined);
		shRenderer.setProjectionMatrix(libGDXcamera.combined);

		Gdx.gl20.glLineWidth(1);

		//set viewport of hud to cover whole window
		HdpiUtils.glViewport(0, 0, Gdx.graphics.getWidth(), Gdx.graphics.getHeight());

		if (controller.getDevTools() != null) {
			controller.getDevTools().render(this);
		}
		
		//render light engine based on first camera
		if (Controller.getLightEngine() != null && !getCameras().isEmpty()) {
			Controller.getLightEngine().render(this, getCameras().get(0).getCenter());
		}

		//render buttons
		stage.draw();
		numSpritesThisFrame = gameSpaceSpriteBatch.getRenderedSprites()-spritesStart;
	}
	
	/**
	 * sets matrix to render in screen space coordinates
	 */
	public void resetProjectionMatrix(){
		gameSpaceSpriteBatch.setProjectionMatrix(libGDXcamera.combined);
		shRenderer.setProjectionMatrix(libGDXcamera.combined);
	}

    /**
     * The equalizationScale is a factor which scales the GUI/HUD to have the same relative size with different resolutions.
     * @return the scale factor
     */
    public float getEqualizationScale() {
		if (initalized)
			return libGDXcamera.viewportWidth / (int) WE.getCVars().get("renderResolutionWidth").getValue();
		else return 1;
    }

   /**
     * Reverts the perspective and transforms it into a coordiante which can be used in the game logic. Should be verified if returning correct results.
     * @param screenX the x position on the screen
     * @param camera the camera where the position is on
     * @return view coordinate
     */
    public float screenXtoView(final int screenX, final Camera camera){
        return screenX / camera.getProjScaling()
			- camera.getScreenPosX()
			+ camera.getViewSpaceX()
			- camera.getWorldWidthViewport()/2;//use left side
    }
    
   /**
     * Reverts the projection and transforms it into game space. Should be verified if returning correct results.
     * @param screenY the y position on the screen. y-up
     * @param camera the camera where the position is on
     * @return view coordinate
     */
    public float screenYtoView(final int screenY, final Camera camera){
        return camera.getViewSpaceY() //to view space
			+ camera.getWorldHeightViewport()/2//use top side, therefore /2
			- screenY / camera.getProjScaling() //to view space and then revert scaling
			- camera.getScreenPosY(); //to projection space
    }
    
    /**
     * Returns matching point on the ground. Can be used in game space but then its on the floor layer.
     * @param x screen space
     * @param y screen space. y-up
     * @return the position on the map. deepest layer. If no camera returns map center.
     */
     public Point screenToGameBasic(final int x, final int y){
		 if (cameras.size() > 0) {
			 //identify clicked camera
			 Camera camera;
			 int i = 0;
			 do {
				 camera = cameras.get(i);
				 i++;
			} while (
				i < cameras.size()
				 && !(x > camera.getScreenPosX()
				 && x < camera.getScreenPosX() + camera.getWidthScreenSpc()
				 && y > camera.getScreenPosY()
				&& y < camera.getScreenPosY()+camera.getHeightScreenSpc())
			);

			 //find points
			 return new Point(
				 screenXtoView(x, camera),
				 screenYtoView(y, camera) * -2,
				 0
			 );
		 } else {
			 return Controller.getMap().getCenter();
		 }
    }
     
    /**
     * Returns the approximated game position belonging to a point on the screen. Does raytracing to find the intersection. Because information is lost if you do game to screen reverting this can only be done by approximating what happens in view -&gt; game. First does screen -&gt; view and then via raytracing view -&gt; game.
	 * 
     * @param x the x position on the screen from left
     * @param y the y position on the screen from bottom
     * @return the position on the map. can return null if no camera available
     */
	public Intersection screenToGame(final int x, final int y) {
		if (cameras.size() > 0) {
			Point p = screenToGameBasic(x, y);
			//find point at top of map
			Vector3 vectorToTop = new Vector3(0, RenderCell.PROJECTIONFACTORZ/RenderCell.PROJECTIONFACTORY, 1).scl((Chunk.getGameHeight()-1));//Vector can be calculated by using an equation where z values are known and top y is unknown and game to view projection is applied. May be a special when Y projection factor is 0.5
			p.add(vectorToTop);//top of map

			return p.rayMarching(
				new Vector3(0, -RenderCell.PROJECTIONFACTORZ, -RenderCell.PROJECTIONFACTORY),//now go in reverse direction
				Float.POSITIVE_INFINITY,
				getRenderStorage(),
				null
			);
		} else {
			return null;
		}
	}
	
	/**
	 * Not a homomorphism, which means f(a*b) != f(a)*f(b)
	 *
	 * @param x view space
	 * @param camera
	 * @return screen space
	 */
	public int viewToScreenX(int x, Camera camera) {
		return (int) (x - camera.getViewSpaceX() + camera.getWidthScreenSpc() / 2);
	}

	/**
	 * Not a homomorphism, which means f(a*b) != f(a)*f(b)
	 *
	 * @param y view space
	 * @param camera
	 * @return screen space
	 */
	public int viewToScreenY(int y, Camera camera) {
		return (int) (y - camera.getViewSpaceY() + camera.getHeightScreenSpc() / 2);
	}

	/**
	 * Draw a string using the color white.
	 *
	 * @param msg
	 * @param xPos screen space
	 * @param yPos screen space
	 * @param openbatch true if begin/end shoould be called
	 */
	public void drawString(final String msg, final int xPos, final int yPos, boolean openbatch) {
		if (openbatch) {
			projectionSpaceSpriteBatch.setProjectionMatrix(libGDXcamera.combined);
			projectionSpaceSpriteBatch.begin();
		}
		WE.getEngineView().getFont().setColor(Color.WHITE.cpy());
		WE.getEngineView().getFont().draw(projectionSpaceSpriteBatch, msg, xPos, yPos);
		if (openbatch) {
			projectionSpaceSpriteBatch.end();
		}
	}
    
    /**
     *Draw a string in a color. Using open batch.
     * @param msg
     * @param xPos screen space
     * @param yPos screen space
     * @param color
     */
    public void drawString(final String msg, final int xPos, final int yPos, final Color color) {
        projectionSpaceSpriteBatch.setColor(Color.WHITE.cpy());
		WE.getEngineView().getFont().setColor(color);
		WE.getEngineView().getFont().draw(projectionSpaceSpriteBatch, msg, xPos, yPos);
    }

    /**
     * to render in screen space with view space scaling?
     * @return
     */
    public ShapeRenderer getShapeRenderer() {
        return shRenderer;
    }
    
    /**
     *
     * @return
     */
    public Controller getController() {
        return controller;
    }
    
     /**
     * Returns a camera. The first camera is handled as the main camera.
     * @return The virtual cameras rendering the scene
     */
    public ArrayList<Camera> getCameras() {
        return cameras;
    }

    /**
     *Get a menu which can be used for loading maps.
     * @return
     */
    public LoadMenu getLoadMenu() {
        if (loadMenu==null) loadMenu = new LoadMenu();//lazy init
        return loadMenu;
    }

    /**
     * Add a camera to the game. Adds this camera to the used {@link RenderStorage}.
     * @param camera
     */
    protected void addCamera(final Camera camera) {
        this.cameras.add(camera);
		GameView.cameraIdCounter++;
		camera.setId(GameView.cameraIdCounter);
		getRenderStorage().addCamera(camera);
    }
    
     /**
     * should be called when the window get resized
     * @param width
     * @param height 
     */
    public void resize(final int width, final int height) {
        for (Camera camera : cameras) {
            camera.resize(width, height);//resizes cameras to fullscreen?
        }
        //stage.setViewport(new StretchViewport(width, height));
        //EngineView.getStage().setViewport(new StretchViewport(width, height));
        libGDXcamera.setToOrtho(false, width, height);
		libGDXcamera.zoom = 1/getEqualizationScale();
        libGDXcamera.update();
    }

    /**
     * The libGDX scene2d stage
     * @return 
     */
    public Stage getStage() {
        return stage;
    }

    /**
     * Game view dependent spriteBatch
     * @return 
     */
    public SpriteBatchWithZAxis getGameSpaceSpriteBatch() {
        return gameSpaceSpriteBatch;
    }
	

	/**
	 * Get the value of projectionSpaceSpriteBatch
	 *
	 * @return the value of projectionSpaceSpriteBatch
	 */
	public SpriteBatch getProjectionSpaceSpriteBatch() {
		return projectionSpaceSpriteBatch;
	}

    /**
     *
     * @return
     */
    @Override
    public boolean isInitalized() {
        return initalized;
    }
	
	/**
	 *
	 */
	public void useDefaultShader(){
		gameSpaceSpriteBatch.setShader(null);
		useDefaultShader = true;
	}

	/**
	 *
	 * @return
	 */
	public boolean isUsingDefaultShader() {
		return useDefaultShader;
	}
	
	/**
	 * 
	 * @param shader 
	 */
	public void setShader(ShaderProgram shader){
		gameSpaceSpriteBatch.setShader(shader);
		useDefaultShader = false;
	}

 
	@Override
	public void exit(){
		//disable cameras
		for (Camera camera : cameras) {
			camera.setActive(false);
		}
	}

	@Override
	public void dispose() {
		for (Camera camera : cameras) {
			camera.dispose();
		}
		if (this.renderstorage != null)
			MessageManager.getInstance().removeListener(this.renderstorage, Events.mapChanged.getId());	
		renderstorage.dispose();
		shRenderer.dispose();
		gameSpaceSpriteBatch.dispose();
		stage.dispose();
		
		cameraIdCounter=0;
	}

	int getRenderedSprites() {
		return numSpritesThisFrame;
	}
	
}