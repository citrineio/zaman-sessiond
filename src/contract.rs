pub const SERVICE: &str = "com.kawnelectro.Zaman.Session1";
pub const PATH: &str = "/com/kawnelectro/Zaman/Session1";
pub const INTERFACE: &str = "com.kawnelectro.Zaman.Session1";
pub const MENU_SERVICE: &str = "com.kawnelectro.Zaman.Menu1";
pub const VERSION: &str = "0.6.5";

pub type StatusTuple = (String, String, String, String, String, String);
pub type MenuStatusTuple = (bool, u64, String);
pub type ForegroundStatusTuple = (String, String, String);

// open, generation, return_target, game_state, pending, error, allowed_actions
pub type MenuContextTuple = (bool, u64, String, String, bool, String, Vec<String>);
