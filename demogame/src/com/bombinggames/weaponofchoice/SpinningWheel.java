package com.bombinggames.weaponofchoice;

import com.badlogic.gdx.Gdx;
import com.badlogic.gdx.backends.lwjgl.audio.Ogg.Sound;
import com.badlogic.gdx.graphics.g2d.Sprite;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.Gameobjects.AbstractGameObject;
import java.util.ArrayList;

/**
 *
 * @author Benedikt Vogler
 */
public class SpinningWheel extends ArrayList<CustomWeapon> {

	private static final long serialVersionUID = 1L;

	private final CustomGameController controller;
	private boolean visible;
	private byte current = -1;
	private final int spintime = 5000;
	private int timer;
	private byte currentRandom;
	private float wheelSpeed;
	private float wheelTimer;

	public SpinningWheel(CustomGameController ctlr) {
		controller = ctlr;
	}

	/**
	 * Returns a new selection
	 */
	public void spin() {
		Sound dudeldi = (Sound) WE.getAsset("com/bombinggames/WeaponOfChoice/Sounds/dudeldi.ogg");
		dudeldi.play();
		WE.getCvars().get("music").setValue(0.2f);

		visible = true;
		timer = spintime;
		WE.getCvars().get("timespeed").setValue(0.3f);
		wheelSpeed = 1;
		wheelTimer = 1;
	}

	public void update(float delta) {
		if (visible) {
			timer -= delta;

			if (timer <= 0) {//reset
				visible = false;
				timer = spintime;
				current = currentRandom;
				controller.getPlayer().equipWeapon(new CustomWeapon(current, controller.getPlayer()));
				WE.getCvars().get("timespeed").setValue(1.0f);
				WE.getCvars().get("music").setValue(1f);
			}

			wheelSpeed *= 1 + delta / 400f;//time to pass before new random item get's bigger

			if (wheelSpeed > 1000) {
				wheelSpeed = 50000;//stop it
			}
			wheelTimer -= delta;
			if (wheelTimer <= 0) {
				wheelTimer = wheelSpeed;
				currentRandom = (byte) (int) (Math.random() * size());
			}
		}
	}

	public void render(GameView view) {
		if (visible) {
			Sprite sprite;
			sprite = new Sprite(AbstractGameObject.getSprite('i', (byte) 5, (byte) 0)); // "canvas")
			sprite.flip(false, true);
			sprite.setX(Gdx.graphics.getWidth() / 2 - sprite.getWidth() / 2);
			sprite.setY(Gdx.graphics.getHeight() / 2 - 30);
			//sprite.scale(CustomWeapon.getScaling());
			sprite.draw(WE.getEngineView().getSpriteBatch());

			if (controller.getRound() == 1) {
				sprite = new Sprite(AbstractGameObject.getSprite('i', (byte) 6, (byte) 0));//warmup
			} else {
				sprite = new Sprite(AbstractGameObject.getSprite('i', (byte) 7, (byte) 0));//newround
			}
			sprite.setX(Gdx.graphics.getWidth() / 2 - sprite.getWidth() / 2);
			sprite.setY(Gdx.graphics.getHeight() / 2 - 200);
			// sprite.scale(CustomWeapon.getScaling());
			sprite.flip(false, true);
			sprite.draw(WE.getEngineView().getSpriteBatch());

			get(currentRandom).renderHUD(
				view,
				Gdx.graphics.getWidth() / 2 - 10,
				Gdx.graphics.getHeight() / 2 - 25
			);
		}
		if (current > -1) {
			get(current).renderHUD(
				view,
				Gdx.graphics.getWidth() - 150,
				Gdx.graphics.getHeight() - 150
			);
		}
	}

	public boolean isVisible() {
		return visible;
	}

	public void setVisible(boolean visible) {
		this.visible = visible;
	}

	@Override
	public Object clone() {
		return super.clone();
	}
}
