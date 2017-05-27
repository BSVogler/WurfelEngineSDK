package com.bombinggames.wurfelengine.core.console;

import com.bombinggames.wurfelengine.core.GameplayScreen;
import java.util.StringTokenizer;

/**
 *
 * @author Benedikt Vogler
 */
public class ReloadShadersCommand implements ConsoleCommand {

	@Override
	public boolean perform(StringTokenizer parameters, GameplayScreen gameplay) {
		gameplay.getView().loadShaders();
		return true;
	}

	@Override
	public String getCommandName() {
		return "reloadshaders";
	}

	/**
	 *
	 * @return
	 */
	@Override
	public String getManual() {
		return "reloads the shaders";
	}
}
