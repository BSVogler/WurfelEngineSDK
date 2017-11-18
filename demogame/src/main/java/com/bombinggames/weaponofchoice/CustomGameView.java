package com.bombinggames.weaponofchoice;

import com.badlogic.gdx.Gdx;
import com.badlogic.gdx.Input;
import com.badlogic.gdx.InputProcessor;
import com.badlogic.gdx.graphics.Color;
import com.badlogic.gdx.graphics.g2d.Sprite;
import com.badlogic.gdx.graphics.glutils.ShapeRenderer;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.Camera;
import com.bombinggames.wurfelengine.core.Controller;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import com.bombinggames.wurfelengine.extension.shooting.Weapon;

/**
 *
 * @author Benedikt
 */
public class CustomGameView extends GameView {

	private final CustomGameController controller;

	/**
	 *
	 * @param controller
	 */
	public CustomGameView(CustomGameController controller) {
		super();
		this.controller = controller;
	}

	@Override
	public void init(Controller controller, GameView oldView) {
		super.init(controller, oldView);
		
		WE.SOUND.register("dudeldi", "com/bombinggames/weaponofchoice/sounds/dudeldi.ogg");
		WE.SOUND.register("reload", "com/bombinggames/weaponofchoice/sounds/reload.wav");
		WE.SOUND.register("shot", "com/bombinggames/weaponofchoice/sounds/shot.wav");
		WE.SOUND.register("melee", "com/bombinggames/weaponofchoice/sounds/melee.wav");
		WE.SOUND.register("punch", "com/bombinggames/weaponofchoice/sounds/punch.wav");
		WE.SOUND.register("shotgun", "com/bombinggames/weaponofchoice/sounds/shotgun.wav");
		WE.SOUND.register("wiz", "com/bombinggames/weaponofchoice/sounds/wiz.wav");
		WE.SOUND.register("poop", "com/bombinggames/weaponofchoice/sounds/poop.wav");
		WE.SOUND.register("thump", "com/bombinggames/weaponofchoice/sounds/thump.wav");
		WE.SOUND.register("fire", "com/bombinggames/weaponofchoice/sounds/fire.wav");
		
		WE.getEngineView().addInputProcessor(new InputListener());
		Camera camera = new Camera(
			this,
			0, //left
			0, //top
			Gdx.graphics.getWidth(), //width 
			Gdx.graphics.getHeight(), //height
			this.controller.getPlayer()
		);

		addCamera(camera);
		this.controller.getPlayer().setCamera(camera);
	}

	@Override
	public void render() {
		super.render();

		getProjectionSpaceSpriteBatch().begin();
			controller.getSpinningWheel().render(this);

			Weapon weapon = controller.getPlayer().getWeapon();
			if (weapon != null) {
				drawString(
					"Shots: " + weapon.getShotsLoaded() + "/" + weapon.getShots(),
					Gdx.graphics.getWidth() - 100,
					100,
					Color.WHITE.cpy()
				);
			}
			Sprite sprite;
			sprite = new Sprite(AbstractGameObject.getSprite('i', (byte) 11, (byte) controller.getPlayer().getWeapon().getWeaponId())); // "canvas")
			sprite.setX(Gdx.graphics.getWidth() - 200);
			sprite.setY(150);
			//sprite.scale(CustomWeapon.getScaling());
			sprite.draw(getProjectionSpaceSpriteBatch());
		getProjectionSpaceSpriteBatch().end();
		
		ShapeRenderer sh = WE.getEngineView().getShapeRenderer();
		//health
		sh.begin(ShapeRenderer.ShapeType.Filled);
		sh.setColor(
			new Color(
				1 - (controller.getPlayer().getHealth() / 1000f),
				controller.getPlayer().getHealth() / 1000f,
				0,
				1
			)
		);
		sh.rect(
			Gdx.graphics.getWidth() / 2 - 100,
			Gdx.graphics.getHeight() - 10,
			controller.getPlayer().getHealth() / 10 * 2,
			50
		);
		sh.end();

		sh.begin(ShapeRenderer.ShapeType.Line);
		sh.setColor(Color.BLACK);
		sh.rect(Gdx.graphics.getWidth() / 2 - 100, Gdx.graphics.getHeight() - 10, 200, 50);
		sh.end();

		//mana
		sh.begin(ShapeRenderer.ShapeType.Filled);
		sh.setColor(
			new Color(
				0,
				0,
				1,
				1
			)
		);
		sh.rect(
			Gdx.graphics.getWidth() / 2 - 100,
			Gdx.graphics.getHeight() - 64,
			controller.getMana() / 10 * 2,
			10
		);
		sh.end();

		sh.begin(ShapeRenderer.ShapeType.Line);
		sh.setColor(Color.BLACK);
		sh.rect(Gdx.graphics.getWidth() / 2 - 100, Gdx.graphics.getHeight() - 10, 200, 50);
		sh.end();

		if (controller.isGameOver()) {
			drawString(
				"Game Over",
				Gdx.graphics.getWidth() / 2 - 30,
				Gdx.graphics.getHeight() / 2 - 170,
				Color.WHITE,
				true
			);

			drawString(
				"Kills:" + Enemy.getKillcounter(),
				Gdx.graphics.getWidth() / 2,
				Gdx.graphics.getHeight() / 2,
				Color.WHITE,
				true
			);
			drawString(
				"You survived " + controller.getSurvivedSeconds() + " seconds.",
				Gdx.graphics.getWidth() / 2,
				Gdx.graphics.getHeight() / 2 + 20,
				Color.WHITE,
				true
			);
		}
	}

	private class InputListener implements InputProcessor {

		@Override
		public boolean keyDown(int keycode) {
			if (!WE.getConsole().isActive()) {
				//toggle fullscreen
				if (keycode == Input.Keys.F) {
					WE.setFullscreen(!Gdx.graphics.isFullscreen());
				}

				//reload
				if (keycode == Input.Keys.R) {
					controller.getPlayer().getWeapon().reload();
				}

				//reset zoom
				if (keycode == Input.Keys.Z) {
					getCameras().get(0).setZoom(1);
					WE.getConsole().add("Zoom reset");
				}

				if (keycode == Input.Keys.ESCAPE) {// Gdx.app.exit();
					WE.showMainMenu();
				}
				
				if (keycode == Input.Keys.NUM_1) {
					controller.getPlayer().equipWeapon(new CustomWeapon((byte) 0, controller.getPlayer()));
				}
				if (keycode == Input.Keys.NUM_2) {
					controller.getPlayer().equipWeapon(new CustomWeapon((byte) 1, controller.getPlayer()));
				}
				if (keycode == Input.Keys.NUM_3) {
					controller.getPlayer().equipWeapon(new CustomWeapon((byte) 2, controller.getPlayer()));
				}
				if (keycode == Input.Keys.NUM_4) {
					controller.getPlayer().equipWeapon(new CustomWeapon((byte) 3, controller.getPlayer()));
				}
				if (keycode == Input.Keys.NUM_5) {
					controller.getPlayer().equipWeapon(new CustomWeapon((byte) 4, controller.getPlayer()));
				}
				if (keycode == Input.Keys.NUM_6) {
					controller.getPlayer().equipWeapon(new CustomWeapon((byte) 5, controller.getPlayer()));
				}
				if (keycode == Input.Keys.NUM_7) {
					controller.getPlayer().equipWeapon(new CustomWeapon((byte) 6, controller.getPlayer()));
				}
				if (keycode == Input.Keys.NUM_8) {
					controller.getPlayer().equipWeapon(new CustomWeapon((byte) 7, controller.getPlayer()));
				}
				
			}

			return true;
		}

		@Override
		public boolean keyUp(int keycode) {
			return false;
		}

		@Override
		public boolean keyTyped(char character) {
			return false;
		}

		@Override
		public boolean touchDown(int screenX, int screenY, int pointer, int button) {
			if (controller.getPlayer().getWeapon() != null) {
				controller.getPlayer().getWeapon().shoot();
				return true;
			} else {
				return false;
			}
		}

		@Override
		public boolean touchUp(int screenX, int screenY, int pointer, int button) {
			return false;
		}

		@Override
		public boolean touchDragged(int screenX, int screenY, int pointer) {
			return false;
		}

		@Override
		public boolean mouseMoved(int screenX, int screenY) {
			return false;
		}

		@Override
		public boolean scrolled(int amount) {
			getCameras().get(0).setZoom(getCameras().get(0).getZoom() - amount / 100f);

			WE.getConsole().add("Zoom: " + getCameras().get(0).getZoom());
			return true;
		}
	}

	@Override
	public CustomGameController getController() {
		return controller;
	}
	
	
}
