#!/usr/bin/env python3
"""
Create a simple sprite atlas from individual block sprites
"""
from PIL import Image
import os

def create_sprite_atlas():
    # Define the sprites we want to include
    sprites = [
        ("demogame/sprites/blocks/b0-0-0.png", "b0-0-0"),  # Air (probably transparent)
        ("demogame/sprites/blocks/b1-0-0.png", "b1-0-0"),  # Grass/Stone
        ("demogame/sprites/blocks/b2-0-0.png", "b2-0-0"),  # Dirt
        ("demogame/sprites/blocks/b8-0-0.png", "b8-0-0"),  # Sand/Stone
    ]
    
    # Load the first sprite to get dimensions
    first_sprite = Image.open(sprites[0][0])
    sprite_width, sprite_height = first_sprite.size
    print(f"Individual sprite size: {sprite_width}x{sprite_height}")
    
    # Create atlas (2x2 grid)
    atlas_width = sprite_width * 2
    atlas_height = sprite_height * 2
    atlas = Image.new("RGBA", (atlas_width, atlas_height), (0, 0, 0, 0))
    
    # Place sprites in atlas
    positions = [(0, 0), (sprite_width, 0), (0, sprite_height), (sprite_width, sprite_height)]
    
    for i, (sprite_path, sprite_name) in enumerate(sprites):
        if os.path.exists(sprite_path):
            sprite_img = Image.open(sprite_path).convert("RGBA")
            x, y = positions[i]
            atlas.paste(sprite_img, (x, y))
            print(f"Placed {sprite_name} at ({x}, {y})")
        else:
            print(f"Warning: {sprite_path} not found")
    
    # Save atlas
    output_path = "wurfel-demo/assets/block_atlas.png"
    os.makedirs(os.path.dirname(output_path), exist_ok=True)
    atlas.save(output_path)
    print(f"Atlas saved to: {output_path}")
    print(f"Atlas size: {atlas_width}x{atlas_height}")
    
    # Print sprite coordinates for reference
    print("\nSprite coordinates in atlas:")
    for i, (_, sprite_name) in enumerate(sprites):
        x, y = positions[i]
        print(f"{sprite_name}: ({x}, {y}, {sprite_width}, {sprite_height})")

if __name__ == "__main__":
    create_sprite_atlas()