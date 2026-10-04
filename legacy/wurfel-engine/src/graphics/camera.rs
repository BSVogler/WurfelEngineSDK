//! Camera system for isometric rendering

use crate::math::{Vec2, Vec3, Vec4, Mat4};

/// Isometric camera for 2.5D rendering
#[derive(Debug, Clone)]
pub struct Camera {
    /// Camera position in world space
    position: Vec3,
    /// Camera target (what it's looking at)
    target: Vec3,
    /// Up vector
    up: Vec3,
    /// Field of view (for orthographic, this is the view height)
    fov: f32,
    /// Aspect ratio (width/height)
    aspect: f32,
    /// Near clipping plane
    near: f32,
    /// Far clipping plane
    far: f32,
    /// View matrix (calculated)
    view_matrix: Mat4,
    /// Projection matrix (calculated)
    projection_matrix: Mat4,
    /// Combined view-projection matrix
    view_proj_matrix: Mat4,
    /// Whether matrices need recalculation
    dirty: bool,
}

impl Camera {
    /// Create a new isometric camera
    pub fn new(aspect: f32) -> Self {
        let mut camera = Self {
            position: Vec3::new(0.0, 0.0, 10.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            fov: 10.0, // View height for orthographic
            aspect,
            near: 0.1,
            far: 1000.0,
            view_matrix: Mat4::IDENTITY,
            projection_matrix: Mat4::IDENTITY,
            view_proj_matrix: Mat4::IDENTITY,
            dirty: true,
        };
        camera.update_matrices();
        camera
    }
    
    /// Set camera position
    pub fn set_position(&mut self, position: Vec3) {
        if self.position != position {
            self.position = position;
            self.dirty = true;
        }
    }
    
    /// Get camera position
    pub fn position(&self) -> Vec3 {
        self.position
    }
    
    /// Set camera target
    pub fn set_target(&mut self, target: Vec3) {
        if self.target != target {
            self.target = target;
            self.dirty = true;
        }
    }
    
    /// Get camera target
    pub fn target(&self) -> Vec3 {
        self.target
    }
    
    /// Set field of view (view height for orthographic)
    pub fn set_fov(&mut self, fov: f32) {
        if (self.fov - fov).abs() > f32::EPSILON {
            self.fov = fov;
            self.dirty = true;
        }
    }
    
    /// Get field of view
    pub fn fov(&self) -> f32 {
        self.fov
    }
    
    /// Set aspect ratio
    pub fn set_aspect(&mut self, aspect: f32) {
        if (self.aspect - aspect).abs() > f32::EPSILON {
            self.aspect = aspect;
            self.dirty = true;
        }
    }
    
    /// Move camera by offset
    pub fn translate(&mut self, offset: Vec3) {
        self.set_position(self.position + offset);
    }
    
    /// Move camera in screen space (useful for panning)
    pub fn pan(&mut self, screen_delta: Vec2) {
        // Convert screen space movement to world space
        let world_delta = Vec3::new(
            screen_delta.x * 0.01, // Scale factor
            0.0,
            screen_delta.y * 0.01,
        );
        self.translate(world_delta);
    }
    
    /// Zoom in/out (changes field of view)
    pub fn zoom(&mut self, factor: f32) {
        self.set_fov(self.fov * factor);
    }
    
    /// Get the view matrix
    pub fn view_matrix(&mut self) -> Mat4 {
        if self.dirty {
            self.update_matrices();
        }
        self.view_matrix
    }
    
    /// Get the projection matrix
    pub fn projection_matrix(&mut self) -> Mat4 {
        if self.dirty {
            self.update_matrices();
        }
        self.projection_matrix
    }
    
    /// Get the combined view-projection matrix
    pub fn view_proj_matrix(&mut self) -> Mat4 {
        if self.dirty {
            self.update_matrices();
        }
        self.view_proj_matrix
    }
    
    /// Update the camera matrices using authentic Wurfel Engine transformation
    fn update_matrices(&mut self) {
        use crate::constants::*;
        
        // Authentic Wurfel Engine camera setup from Camera.java lines 327-354
        
        // Create orthographic projection (lines 327-334 in original)
        let width = self.fov * self.aspect;
        let height = self.fov;
        
        self.projection_matrix = Mat4::orthographic_rh(
            width / 2.0,    // left (swapped from original for RH)
            -width / 2.0,   // right  
            height / 2.0,   // top
            -height / 2.0,  // bottom
            1.0,            // near
            2200.0,         // far (from original)
        );
        
        // Reverse z for better fit with near and far plane (line 336 in original)
        self.projection_matrix.z_axis.z *= -1.0;
        
        // Set up view-projection matrix 
        self.view_proj_matrix = self.projection_matrix;
        
        // Create view matrix (lines 342-346 in original)
        // lookAt(position(x,y,1), target(x,y,-1), up(0,-1,0))
        self.view_matrix = Mat4::look_at_rh(
            Vec3::new(self.position.x, self.position.y, 1.0),
            Vec3::new(self.position.x, self.position.y, -1.0),
            Vec3::new(0.0, -1.0, 0.0),
        );
        
        // Apply view matrix transformation (Matrix4.mul in original)
        self.view_proj_matrix = self.view_proj_matrix * self.view_matrix;
        
        // CRITICAL: Apply authentic Wurfel Engine isometric transformation (lines 351-354)
        
        // Line 352: viewprojection.val[M12] = viewprojection.val[M11] * PROJECTIONFACTORZ
        // M12 is row 1, col 2 (y row, z col) - this projects Z into Y  
        let m11_value = self.view_proj_matrix.x_axis.x; // M11 in original
        self.view_proj_matrix.y_axis.z = m11_value * PROJECTIONFACTORZ;
        
        // Line 354: viewprojection.val[M11] *= -PROJECTIONFACTORY  
        // M11 is row 1, col 1 (y row, y col) - this flips and scales Y projection
        self.view_proj_matrix.y_axis.y *= -PROJECTIONFACTORY;
        
        self.dirty = false;
    }
    
    /// Convert world coordinates to screen coordinates
    pub fn world_to_screen(&mut self, world_pos: Vec3) -> Vec2 {
        let clip_pos = self.view_proj_matrix() * world_pos.extend(1.0);
        let ndc = Vec2::new(clip_pos.x / clip_pos.w, clip_pos.y / clip_pos.w);
        
        // Convert from NDC (-1 to 1) to screen coordinates
        // Assuming standard screen dimensions - this will need to be updated
        // when we have access to actual screen dimensions
        let screen_width = 1024.0;
        let screen_height = 768.0;
        
        Vec2::new(
            (ndc.x + 1.0) * 0.5 * screen_width,
            (1.0 - ndc.y) * 0.5 * screen_height, // Flip Y for screen coordinates
        )
    }
    
    /// Convert screen coordinates to world coordinates (on the z=0 plane)
    pub fn screen_to_world(&mut self, screen_pos: Vec2) -> Vec3 {
        let inv_matrix = self.view_proj_matrix().inverse();
        let world_pos = inv_matrix * Vec4::new(screen_pos.x, screen_pos.y, 0.0, 1.0);
        Vec3::new(
            world_pos.x / world_pos.w,
            world_pos.y / world_pos.w,
            world_pos.z / world_pos.w,
        )
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::new(1.0)
    }
}

/// Camera controller for user input
#[derive(Debug)]
pub struct CameraController {
    /// Movement speed
    pub move_speed: f32,
    /// Zoom speed
    pub zoom_speed: f32,
    /// Current input state
    pub input_state: CameraInputState,
}

#[derive(Debug, Default)]
pub struct CameraInputState {
    pub move_left: bool,
    pub move_right: bool,
    pub move_up: bool,
    pub move_down: bool,
    pub zoom_in: bool,
    pub zoom_out: bool,
    pub mouse_delta: Vec2,
}

impl CameraController {
    pub fn new() -> Self {
        Self {
            move_speed: 5.0,
            zoom_speed: 1.1,
            input_state: CameraInputState::default(),
        }
    }
    
    /// Update camera based on current input state
    pub fn update(&self, camera: &mut Camera, dt: f32) {
        let mut movement = Vec3::ZERO;
        
        if self.input_state.move_left {
            movement.x -= 1.0;
        }
        if self.input_state.move_right {
            movement.x += 1.0;
        }
        if self.input_state.move_up {
            movement.z += 1.0;
        }
        if self.input_state.move_down {
            movement.z -= 1.0;
        }
        
        if movement.length() > 0.0 {
            movement = movement.normalize() * self.move_speed * dt;
            camera.translate(movement);
        }
        
        if self.input_state.zoom_in {
            camera.zoom(1.0 / self.zoom_speed);
        }
        if self.input_state.zoom_out {
            camera.zoom(self.zoom_speed);
        }
        
        // Handle mouse panning
        if self.input_state.mouse_delta.length() > 0.0 {
            camera.pan(self.input_state.mouse_delta * dt);
        }
    }
}

impl Default for CameraController {
    fn default() -> Self {
        Self::new()
    }
}