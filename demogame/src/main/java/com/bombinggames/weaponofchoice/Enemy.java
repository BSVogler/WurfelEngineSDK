package com.bombinggames.weaponofchoice;

import com.bombinggames.wurfelengine.core.gameobjects.MovableEntity;

/**
 * An enemy which can follow a character.
 *
 * @author Benedikt Vogler
 */
public class Enemy extends MovableEntity {

	private static final long serialVersionUID = 1L;
	private static int killcounter = 0;
	private final EnemyAI ai;

	public void init() {
		killcounter = 0;
	}

	/**
	 * Zombie constructor.

	 */
	public Enemy() {
		super((byte) 44, 2);
		setDamageSounds(new String[]{"impactFlesh"});
		ai = new EnemyAI();
		
		addComponent(ai);
	}
	
	/**
	 * @param target
	 */
	public void setTarget(MovableEntity target){
		ai.setTarget(target);
	}

	@Override
	public void jump() {
		jump(5, true);
	}

	@Override
	public void dispose() {
		killcounter++;
		super.dispose();
	}

	public static int getKillcounter() {
		return killcounter;
	}
}
