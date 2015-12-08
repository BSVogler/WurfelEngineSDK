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

import com.badlogic.gdx.ai.pfa.Connection;
import com.badlogic.gdx.utils.Array;

/** The default implementation of a graph for the {@link IndexedAStarPathFinder} that uses an internal {@link Array} to store
 * nodes.
 * 
 * @param <N> Type of node extending {@link IndexedNode}
 * 
 * @author davebaol */
public class DefaultIndexedGraph<N extends IndexedNode<N>> implements IndexedGraph<N> {

	protected Array<N> nodes;

	/** Creates an {@code IndexedGraph} with no nodes. */
	public DefaultIndexedGraph () {
		this(new Array<N>());
	}

	/** Creates an {@code IndexedGraph} with the given capacity and no nodes. */
	public DefaultIndexedGraph (int capacity) {
		this(new Array<N>(capacity));
	}

	/** Creates an {@code IndexedGraph} with the given nodes. */
	public DefaultIndexedGraph (Array<N> nodes) {
		this.nodes = nodes;
	}

	@Override
	public Array<Connection<N>> getConnections (N fromNode) {
		return nodes.get(fromNode.getIndex()).getConnections();
	}

	@Override
	public int getNodeCount () {
		return nodes.size;
	}
}
