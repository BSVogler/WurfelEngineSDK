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
package com.bombinggames.wurfelengine.core.sorting;

import com.bombinggames.wurfelengine.core.Camera;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import java.util.LinkedList;

/**
 *
 * @author Benedikt Vogler
 */
public class DepthValueSort extends AbstractSorter {

	private final NoSort nosorter;
	private LinkedList<AbstractGameObject> depthlist = new LinkedList<>();

	public DepthValueSort(Camera camera) {
		super(camera);
		nosorter = new NoSort(camera);
		
	}

	@Override
	public void createDepthList(LinkedList<AbstractGameObject> depthlist) {
		nosorter.createDepthList(depthlist);
		depthlist.sort((AbstractGameObject o1, AbstractGameObject o2) -> {
			float d1 = o1.getDepth();
			float d2 = o2.getDepth();
			if (d1 > d2) {
				return 1;
			} else {
				if (d1 == d2) {
					return 0;
				}
				return -1;
			}
		});
	}

	@Override
	public void renderSorted() {
		createDepthList(depthlist);
		for (AbstractGameObject abstractGameObject : depthlist) {
			abstractGameObject.render(camera.getGameView());
		}
	}
	
}
