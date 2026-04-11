use std::fmt;

use serde::{Deserialize, Serialize, de};
use types::{
    Function,
    LuaRef,
    Object,
    conversion::{self, FromObject, ToObject},
    serde::Serializer,
};

/// See `:h command-complete` for details.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandComplete {
    Arglist,
    Augroup,
    Buffer,
    Behave,
    Color,
    Command,
    Compiler,
    Cscope,
    Dir,
    Environment,
    Event,
    Expression,
    File,
    FileInPath,
    Filetype,
    Function,
    Help,
    Highlight,
    History,
    Locale,
    Lua,
    Mapclear,
    Mapping,
    Menu,
    Messages,
    Option,
    Packadd,
    Shellcmd,
    Sign,
    Syntax,
    Syntime,
    Tag,
    TagListfiles,
    User,
    Var,

    /// See `:h command-completion-customlist` for details.
    CustomList(Function<(String, String, usize), Vec<String>>),
}

impl ToObject for CommandComplete {
    fn to_object(self) -> Result<Object, conversion::Error> {
        self.serialize(Serializer::new()).map_err(Into::into)
    }
}

impl<'de> Deserialize<'de> for CommandComplete {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct CommandCompleteVisitor;

        impl<'de> de::Visitor<'de> for CommandCompleteVisitor {
            type Value = CommandComplete;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a command-complete string or Lua callback")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                use CommandComplete::*;

                match value {
                    "arglist" => Ok(Arglist),
                    "augroup" => Ok(Augroup),
                    "buffer" => Ok(Buffer),
                    "behave" => Ok(Behave),
                    "color" => Ok(Color),
                    "command" => Ok(Command),
                    "compiler" => Ok(Compiler),
                    "cscope" => Ok(Cscope),
                    "dir" => Ok(Dir),
                    "environment" => Ok(Environment),
                    "event" => Ok(Event),
                    "expression" => Ok(Expression),
                    "file" => Ok(File),
                    "file_in_path" => Ok(FileInPath),
                    "filetype" => Ok(Filetype),
                    "function" => Ok(Function),
                    "help" => Ok(Help),
                    "highlight" => Ok(Highlight),
                    "history" => Ok(History),
                    "locale" => Ok(Locale),
                    "lua" => Ok(Lua),
                    "mapclear" => Ok(Mapclear),
                    "mapping" => Ok(Mapping),
                    "menu" => Ok(Menu),
                    "messages" => Ok(Messages),
                    "option" => Ok(Option),
                    "packadd" => Ok(Packadd),
                    "shellcmd" => Ok(Shellcmd),
                    "sign" => Ok(Sign),
                    "syntax" => Ok(Syntax),
                    "syntime" => Ok(Syntime),
                    "tag" => Ok(Tag),
                    "tag_listfiles" => Ok(TagListfiles),
                    "user" => Ok(User),
                    "var" => Ok(Var),
                    other => Err(E::unknown_variant(
                        other,
                        &[
                            "arglist",
                            "augroup",
                            "buffer",
                            "behave",
                            "color",
                            "command",
                            "compiler",
                            "cscope",
                            "dir",
                            "environment",
                            "event",
                            "expression",
                            "file",
                            "file_in_path",
                            "filetype",
                            "function",
                            "help",
                            "highlight",
                            "history",
                            "locale",
                            "lua",
                            "mapclear",
                            "mapping",
                            "menu",
                            "messages",
                            "option",
                            "packadd",
                            "shellcmd",
                            "sign",
                            "syntax",
                            "syntime",
                            "tag",
                            "tag_listfiles",
                            "user",
                            "var",
                        ],
                    )),
                }
            }

            fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let callback = Function::from_object(Object::from_luaref(
                    value as LuaRef,
                ))
                .map_err(E::custom)?;

                Ok(CommandComplete::CustomList(callback))
            }
        }

        deserializer.deserialize_any(CommandCompleteVisitor)
    }
}

#[cfg(test)]
mod tests {
    use types::{Object, serde::Deserializer};

    use super::*;

    #[test]
    fn test_command_complete_deserializes_builtin_variant() {
        let complete = CommandComplete::deserialize(Deserializer::new(
            Object::from("file"),
        ))
        .unwrap();
        assert_eq!(CommandComplete::File, complete);
    }

    #[test]
    fn test_command_complete_deserializes_lua_callback() {
        let complete = CommandComplete::deserialize(Deserializer::new(
            Object::from_luaref(77),
        ))
        .unwrap();

        let CommandComplete::CustomList(callback) = complete else {
            panic!("expected callback completion");
        };

        assert_eq!(77, callback.lua_ref());
    }
}
