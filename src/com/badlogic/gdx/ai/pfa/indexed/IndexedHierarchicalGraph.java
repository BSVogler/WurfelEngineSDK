/*******************************************************************************
 * Copyright 2014 See AUTHORS file.
 * 
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 * 
 *   http://www.apache.org/licenses/LICENSE-2.0
 * 
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 ******************************************************************************/

package com.badlogic.gdx.ai.pfa.indexed;

import com.badlogic.gdx.ai.pfa.HierarchicalGraph;
import com.badlogic.gdx.utils.Array;

/** A graph for the {@link IndexedAStarPathFinder} that uses an internal {@link Array} to store nodes.
 * 
 * @param <N> Type of node extending {@link IndexedNode}
 * 
 * @author davebaol */
public abstract class IndexedHierarchicalGraph<N extends IndexedNode<N>> extends DefaultIndexedGraph<N> implements HierarchicalGraph<N> {

	protected int levelCount;
	protected int level;

	/** Creates an empty {@code IndexedGraph} with the given number of levels. */
	public IndexedHierarchicalGraph (int levelCount) {
		this(levelCount, new Array<N>());
	}

	/** Creates an empty {@code IndexedHierarchicalGraph} with the given number of levels and capacity. */
	public IndexedHierarchicalGraph (int levelCount, int capacity) {
		this(levelCount, new Array<N>(capacity));
	}

	/** Creates an {@code IndexedHierarchicalGraph} with the given number of levels and containing the given nodes. */
	public IndexedHierarchicalGraph (int levelCount, Array<N> nodes) {
		super(nodes);
		this.levelCount = levelCount;
		this.level = 0;
	}

	@Override
	public int getLevelCount () {
		return levelCount;
	}

	@Override
	public void setLevel (int level) {
		this.level = level;
	}

	@Override
	public abstract N convertNodeBetweenLevels (int inputLevel, N node, int outputLevel);

}
