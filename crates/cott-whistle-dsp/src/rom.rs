//! Factory paddle table: [`Voice::recipe`] is the 2701 ROM, nothing else.
//!
//! Numbers live in [`crate::rom_bits`]. This file is the Voice → column map
//! so the rest of the crate keeps calling `voice.recipe()`.

use crate::recipe::Recipe;
use crate::rom_bits;
use crate::voice::Voice;

impl Voice {
    pub fn recipe(self) -> Recipe {
        rom_bits::recipe_from_rom(self)
    }
}
