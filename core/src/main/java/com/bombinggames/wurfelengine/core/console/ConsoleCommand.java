package com.bombinggames.wurfelengine.core.console;

import java.util.StringTokenizer;

import com.bombinggames.wurfelengine.core.GameplayScreen;

/**
 *
 * @author Benedikt Vogler
 */
public interface ConsoleCommand {
	
	/**
	 * 
	 * @param parameters the value of parameters
	 * @param gameplay the value of gameplay
	 * @return the boolean 
	 */
	public abstract boolean perform(StringTokenizer parameters, GameplayScreen gameplay);
	
	/**
	 * always lowercase. The name is the identifier of this command.
	 * @return 
	 */
	public abstract String getCommandName();
	
	/**
	 *
	 * @return
	 */
	public abstract String getManual();
}
