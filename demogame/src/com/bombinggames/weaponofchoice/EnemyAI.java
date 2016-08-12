/*
 * To change this license header, choose License Headers in Project Properties.
 * To change this template file, choose Tools | Templates
 * and open the template in the editor.
 */
package com.bombinggames.weaponofchoice;

import com.badlogic.gdx.Gdx;
import com.badlogic.gdx.ai.msg.MessageManager;
import com.badlogic.gdx.ai.msg.Telegram;
import com.badlogic.gdx.ai.msg.Telegraph;
import com.bombinggames.wurfelengine.core.Events;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractEntity;
import com.bombinggames.wurfelengine.core.gameobjects.Component;
import com.bombinggames.wurfelengine.core.gameobjects.MovableEntity;
import com.bombinggames.wurfelengine.core.map.Point;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;

/**
 *
 * @author Benedikt Vogler
 */
public class EnemyAI implements Component, Telegraph {

	private MovableEntity body;
	private int mana = 0;
	private final Point lastPos = new Point(0, 0, 0);
	private int runningagainstwallCounter = 0;
	private MovableEntity target;

	@Override
	public void update(float dt) {
		if (body.hasPosition() && body.getPosition().isInMemoryAreaXY()) {
			//follow the target
			if (target != null && target.hasPosition()) {
				if (body.getPosition().distanceTo(target) > RenderCell.GAME_EDGELENGTH * 1.5f) {
					MessageManager.getInstance().dispatchMessage(
						this,
						body,
						Events.moveTo.getId(),
						target.getPosition()
					);
					body.setSpeedHorizontal(0.4f);
				} else {
					MessageManager.getInstance().dispatchMessage(
						this,
						body,
						Events.standStill.getId()
					);
				}
				mana = ((int) (mana + dt));
				if (mana >= 1000) {
					mana = 0;//reset
					//new EntityAnimation(new int[]{300}, true, false)
					//new EntityAnimation(46, 0, new int[]{300}, true, false).spawn(getPosition().cpy());//spawn blood
					MessageManager.getInstance().dispatchMessage(
						body,
						target,
						Events.damage.getId(),
						(byte) 50
					);
				}
			}

			//if standing on same position as in last update
			if (body.getPoint().equals(lastPos)) {
				runningagainstwallCounter += dt;
			} else {
				runningagainstwallCounter = 0;
				lastPos.set(body.getPosition());
			}

			//jump after some time
			if (runningagainstwallCounter > 500 && body.isOnGround()) {
				MessageManager.getInstance().dispatchMessage(
					this,
					target,
					Events.damage.getId(),
					(byte) 50
				);
				mana = 0;
				runningagainstwallCounter = 0;
			}
		}
	}

	/**
	 * Set the target which the zombie follows.
	 *
	 * @param target an character
	 */
	public void setTarget(MovableEntity target) {
		this.target = target;
	}

	@Override
	public void setParent(AbstractEntity body) {
		if (body instanceof MovableEntity) {
			this.body = (MovableEntity) body;
		} else {
			Gdx.app.error("EnemyAi", "body must be from type " + MovableEntity.class.getTypeName());
		}
	}

	@Override
	public void dispose() {
	}

	@Override
	public boolean handleMessage(Telegram msg) {
		return false;
	}

}
