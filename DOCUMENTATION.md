# Introduction

## This documentation will talk about the following

- [Information about the Application](#introduction-to-the-application)
- The script API
  - Utility functions
  - Controlling the drawer(s)
  - Interacting with the user

Examples will be show thorughout the documentation to help the reader.

### Introduction to the Application

Ferris Draw is a versatile, user-friendly application designed to help people get familiar with programming. More specifically, with the lua programming language.

The application strives to be a usable tool for all ages.

It is written in the Rust programming language, using bevy as it's game engine and Egui + Eframe for its GUI framework and library.

### Introduction to the User Interface

Before beginning production of the most beautiful drawings, one must know what they're working with.

**The next image depicts the main navigation bar of the application.**

![topbar_image](assets/documentation/topbar.png)

- Blue (File): Opens up the file menu where the user can save and load their projects. All projects use the `.data` extenstion. These save files are serde serialized and compressed.
- Red (Toolbox): Opens up the toolbox menu where different parts of the ui can be enabled or disabled.
- Yellow (Documentation): Opens up the documentation window in the Application.

**The different parts of the User Interface:**

![uiparts_image](assets/documentation/ui_parts.png)

- Yellow (Top Bar): This part is to control the main functionality of the Application and to customize the environment.
- Blue (Command Panel): You can enter commands in the Command Panel to execute them quickly, without having to write / create a new script. The user can see the input (what they've entered) and the output the Lua runtime returned. You can re-enter your input history via the Up and Down button. Commands can be sent with pressing Enter.
- Red (Item Manager): The item manager consists of different tabs all for displaying different information:
  - The `Entities` tab is used to display the currently available [Drawers](#drawers-tab).
  - The `Scripts` tab is used to display the currently existing [Scripts](#scripts-tab) and the deleted scripts in the rubbish bin.
- Green (Canvas): The canvas is where the user can draw freely. This part of the UI is managed purely by the bevy game engine.

## Introduction to the Scripting API

Scripting in this Application is made possible by [mlua](https://github.com/mlua-rs/mlua), therefor it is running lua as it's programming language.
There will be examples to most of the functions listed.

### Utility functions

1. **`new(String)`**  
   Creates a new drawer object with the specified name.

2. **`center(String)`**  
   Centers the object identified by the given name.

3. **`wipe()`**
   Wipes all drawings from the workspace.

4. **`exists(String)`**
   Returns whether the drawer exists with that specific ID.

5. **`remove(String)`**
   Removes the drawer object based on the ID.

6. **`drawers()`**
   Returns a list of the drawers' name.

**The example usage of these functions.**

```lua
-- If drawer1 doesnt exist then we should create one.
-- Drawer1 will show up in the Entities tab.
if not exists("drawer1") then
    new("drawer1")
end

-- This will return a list of the drawer's name
drawers() --Output: ["drawer1"]

--[[
    **Drawing with the drawer**
]]

-- Drawer1 will return to its original position (0, 0, 0).
center("drawer1")

-- Deleted all of the drawings of all of the drawers.
wipe()

-- Removes the drawer based on the ID
remove("drawer1")

-- This will return a list of the drawer's name
drawers() --Output: [] (The list is empty because there arent any drawers)
```

### Graphical functions

2. **`rotate(String, f32)`**  
   Rotates the object identified by the given name by a specified angle (`f32`). The angle is in degrees.

3. **`forward(String, f32)`**  
   Moves the object identified by the given name forward by the specified distance (`f32`). The direction of movement depends on the current orientation of the object.

5. **`color(String, f32, f32, f32, f32)`**  
   Sets the color of the object identified by the given name. The parameters `f32, f32, f32, f32` represent red, green, blue, and alpha (opacity) values, each ranging from 0.0 to 1.0.
