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
import com.badlogic.gdx.InputMultiplexer;
import com.badlogic.gdx.InputProcessor;
import com.badlogic.gdx.graphics.Color;
import com.badlogic.gdx.graphics.Cursor;
import com.badlogic.gdx.graphics.Pixmap;
import com.badlogic.gdx.graphics.g2d.BitmapFont;
import com.badlogic.gdx.graphics.g2d.SpriteBatch;
import com.badlogic.gdx.graphics.glutils.ShapeRenderer;
import com.badlogic.gdx.scenes.scene2d.Stage;
import com.badlogic.gdx.scenes.scene2d.ui.Skin;
import com.badlogic.gdx.utils.viewport.StretchViewport;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.mapeditor.EditorToggler;

/**
 * A view which is not dependend on the currently active game. It can therefore be used to render in screen space and in the menus. Only one instance
 * should be used.
 *
 * @author Benedikt Vogler
 * @since 1.2.26
 */
public class EngineView {

	private static EngineView instance;
	
	public static EngineView getInstance(){
		if (instance==null)
			instance = new EngineView();
		return instance;
	}
	
	private final BitmapFont font = new BitmapFont(false);
	private final Skin skin = new Skin(Gdx.files.internal("com/bombinggames/wurfelengine/core/skin/uiskin.json"));
	private Cursor cursor;
	private Cursor cursorDrag;
	private Cursor cursorPointer;
	private int cursorId;
	private final EditorToggler editorToggler = new EditorToggler();
	private final ShapeRenderer shRenderer = new ShapeRenderer();
	private final SpriteBatch spriteBatch = new SpriteBatch(2000);
	private final Stage stage = new Stage(
			new StretchViewport(
				Gdx.graphics.getWidth(),
				Gdx.graphics.getHeight()
			),
			spriteBatch
		);//spawn at fullscreen;
	private InputProcessor inactiveInpProcssrs;

	/**
	 * Iniatializes.
	 *
	 * @param controller
	 * @param oldView
	 */
	private EngineView() {
		Gdx.app.debug("EngineView", "Initializing...");
		//set up font
		//font = WurfelEngine.getInstance().manager.get("com/bombinggames/wurfelengine/EngineCore/arial.fnt"); //load font
		//font.scale(2);

		font.setColor(Color.GREEN);
		//font.scale(-0.5f);

		//getSpriteBatch().setProjectionMatrix(camera.combined);
		//getShapeRenderer().setProjectionMatrix(camera.combined);

		Gdx.input.setInputProcessor(stage);

		//spriteBatch.setProjectionMatrix(libGDXcamera.combined);
		//shRenderer.setProjectionMatrix(libGDXcamera.combined);
		//spriteBatch.setTransformMatrix(new Matrix4());//reset transformation
		//shRenderer.setTransformMatrix(new Matrix4());//reset transformation
	}

	/**
	 * The libGDX scene2d stage
	 *
	 * @return
	 */
	public Stage getStage() {
		return stage;
	}

	/**
	 *
	 * @param dt
	 */
	public void update(float dt) {
		editorToggler.setVisible(WE.getCVars().getValueB("editorVisible"));
		editorToggler.update(this, dt);
	}

	/**
	 * render in screen space
	 *
	 * @return
	 */
	public SpriteBatch getSpriteBatch() {
		return spriteBatch;
	}

	/**
	 * to render in screen space with view space scaling?
	 *
	 * @return
	 */
	public ShapeRenderer getShapeRenderer() {
		return shRenderer;
	}

	/**
	 * The equalizationScale is a factor which scales the GUI/HUD to have the
	 * same relative size with different resolutions.
	 *
	 * @return the scale factor
	 */
	public float getEqualizationScale() {
		return Gdx.graphics.getWidth() / (int) WE.getCVars().get("renderResolutionWidth").getValue();
	}

	/**
	 * Resets the input processors.
	 */
	public void resetInputProcessors() {
		Gdx.input.setInputProcessor(getStage());
		inactiveInpProcssrs = null;
		addInputProcessor(stage);
	}

	/**
	 * Add an inputProcessor to the views.
	 *
	 * @param processor
	 */
	public void addInputProcessor(final InputProcessor processor) {
		InputMultiplexer inpMulPlex = new InputMultiplexer(Gdx.input.getInputProcessor());
		inpMulPlex.addProcessor(processor);
		Gdx.input.setInputProcessor(inpMulPlex);
	}

	/**
	 * Deactivates every input processor but one.
	 *
	 * @param processor the processor you want to "filter"
	 * @see #unfocusInputProcessor()
	 * @since V1.2.21
	 */
	public void focusInputProcessor(final InputProcessor processor) {
		inactiveInpProcssrs = Gdx.input.getInputProcessor();//save current ones
		Gdx.input.setInputProcessor(stage); //reset
		addInputProcessor(processor);//add the focus
	}

	/**
	 * Reset that every input processor works again.
	 *
	 * @see #focusInputProcessor(com.badlogic.gdx.InputProcessor)
	 * @since V1.2.21
	 */
	public void unfocusInputProcessor() {
		Gdx.input.setInputProcessor(stage); //reset
		addInputProcessor(inactiveInpProcssrs);
	}

	/**
	 *
	 * @return
	 */
	public BitmapFont getFont() {
		return font;
	}

	/**
	 *
	 * @return
	 */
	public Skin getSkin() {
		return skin;
	}

	/**
	 *
	 * @param id 0 default, 1 pointer, 2 drag
	 */
	public void setCursor(int id) {
		cursorId = id;
		switch (id) {
			case 1:
				if (cursorPointer == null) {
					cursorPointer = Gdx.graphics.newCursor(new Pixmap(Gdx.files.internal("com/bombinggames/wurfelengine/core/images/wecursor.png")), 0, 0);
				}
				Gdx.graphics.setCursor(cursorPointer);
				break;
			case 2:
				if (cursorDrag == null) {
					cursorDrag = Gdx.graphics.newCursor(new Pixmap(Gdx.files.internal("com/bombinggames/wurfelengine/core/images/cursor_drag.png")), 0, 0);
				}
				Gdx.graphics.setCursor(cursorDrag);
				break;
			case 0:
			default:
				if (cursor == null) {
					cursor = Gdx.graphics.newCursor(
						new Pixmap(Gdx.files.internal("com/bombinggames/wurfelengine/core/images/cursor.png")),
						8,
						8
					);
				}
				Gdx.graphics.setCursor(cursor);
				break;
		}
	}

	/**
	 * returns the current cursor
	 *
	 * @return 0 default, 1 pointer, 2 drag
	 */
	public int getCursor() {
		return cursorId;
	}

	/**
	 *
	 * @return
	 */
	public EditorToggler getEditorToggler() {
		return editorToggler;
	}
}
