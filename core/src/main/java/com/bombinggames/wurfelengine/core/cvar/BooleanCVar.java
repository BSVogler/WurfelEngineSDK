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
package com.bombinggames.wurfelengine.core.cvar;

import com.badlogic.gdx.Gdx;

/**
 *
 * @author Benedikt Vogler
 */
public class BooleanCVar extends CVar {

	private boolean value;
	private Boolean defaultValue;

	/**
	 *
	 * @param value
	 */
	public BooleanCVar(boolean value) {
		this.value = value;
		this.defaultValue = value;
	}

	/**
	 *
	 * @return
	 */
	@Override
	public Boolean getValue() {
		return value;
	}

	/**
	 *
	 * @param value
	 */
	@Override
	public void setValue(Object value) {
		if (value instanceof String) {
			this.value = value.equals("1");
		} else {
			this.value = (boolean) value;
		}
		if (flags == CVarFlags.ARCHIVE && Gdx.files != null) {
			parent.save();
		}
	}

	@Override
	public String toString() {
		if (value) {
			return "1";
		} else {
			return "0";
		}
	}

	/**
	 *
	 * @return
	 */
	@Override
	public Boolean getDefaultValue() {
		return defaultValue;
	}

	/**
	 *
	 * @param defaultValue
	 */
	@Override
	public void setDefaultValue(Object defaultValue) {
		this.defaultValue = (Boolean) defaultValue;
	}

}
