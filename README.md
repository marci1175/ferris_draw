## Ferris draw
- An educational tool aimed to teach the basics of programming to children.

<p align="center">
  <img src="https://github.com/marci1175/ferris_draw/blob/master/assets/icon.png" alt="Application Icon" width="400">
</p>

### Project
- This project was originally an [Imagine Logo](https://imagine.input.sk/international.html) port to rust.
- The reason I am creating this project is because I had a ton of fun with it when I was in elemntary school learning about this.

### Project Features
- The project uses [lua](https://www.lua.org/) as its programming language, with [mlua](https://github.com/mlua-rs/mlua).
- You can import any library in your script, as the [lua](https://www.lua.org/) instance to support external libraries.
- The drawings are rendered with the [Bevy game engine](https://bevyengine.org/), to ensure flexibility, safety and speed.
- Ui components are created via [egui](https://crates.io/crates/egui).
- You can run scripts by utilizing the scripts tab but you can run quick commands via the Command Panel available in the application.
  - Syntax highlighting is available in the Script Manager and the Command Panel.
- The Command Panel has the user friendly interface with features to enhance production.
- You can also save and open projects.

### Drawing Capabilities
- Create multiple drawers (with a unique ID).
- Draw lines forward and backward.
- Rotate the drawer to left and right.
- Set the color of the lines the user is drawing.

### Documentation

1. **`new(String)`**  
   Creates a new drawer object with the specified name.

2. **`rotate(String, f32)`**  
   Rotates the object identified by the given name by a specified angle (`f32`). The angle is in degrees.

3. **`forward(String, f32)`**  
   Moves the object identified by the given name forward by the specified distance (`f32`). The direction of movement depends on the current orientation of the object.

4. **`center(String)`**  
   Centers the object identified by the given name.

5. **`color(String, f32, f32, f32, f32)`**  
   Sets the color of the object identified by the given name. The parameters `f32, f32, f32, f32` represent red, green, blue, and alpha (opacity) values, each ranging from 0.0 to 1.0.

6. **`wipe()`**
   Wipes all drawings from the workspace.