use std::env::current_dir;
use crate::OrbitResult;
use clif::cmd::{FromCli, Command};
use crate::core::pkgid::PkgPart;
use crate::core::version::{AnyVersion, self};
use crate::core::lang::vhdl::primaryunit::PrimaryUnit;
use clif::Cli;
use clif::arg::{Flag, Optional};
use clif::Error as CliError;
use crate::core::context::Context;
use crate::util::anyerror::AnyError;
use crate::util::anyerror::Fault;
use crate::core::v2::ip::Ip;
use crate::core::v2::catalog::Catalog;

#[derive(Debug, PartialEq)]
pub struct Show {
    ip: Option<PkgPart>,
    tags: bool,
    units: bool,
    version: Option<AnyVersion>,
}

impl FromCli for Show {
    fn from_cli<'c>(cli: &'c mut Cli) -> Result<Self,  CliError> {
        cli.check_help(clif::Help::new().quick_text(HELP).ref_usage(2..4))?;
        let command = Ok(Show {
            tags: cli.check_flag(Flag::new("versions"))?,
            units: cli.check_flag(Flag::new("units"))?,
            version: cli.check_option(Optional::new("ver").switch('v').value("version"))?,
            ip: cli.check_option(Optional::new("ip").value("name"))?,
        });
        command
    }
}

impl Command<Context> for Show {
    type Status = OrbitResult;

    fn exec(&self, c: &Context) -> Self::Status {
        
        // collect all manifests available (load catalog)
        let catalog = Catalog::new()
            .installations(c.get_cache_path())?;

        // try to auto-determine the ip (check if in a working ip)
        let ip_path: std::path::PathBuf = if let Some(name) = &self.ip {
            // find the path to the provided ip by searching through the catalog
            if let Some(lvl) = catalog.inner().get(name) {
                // return the highest available version
                let spec_ver = self.version.as_ref().unwrap_or(&AnyVersion::Latest);
                if let Some(slot) = lvl.get(spec_ver, true) {
                    slot.get_root().clone()
                } else {
                    return Err(AnyError(format!("the requested ip is not installed")))?
                }
            } else {
                return Err(AnyError(format!("no ip found in cache")))?
            }
        } else {
            let ip = Context::find_ip_path(&current_dir().unwrap());  
            if ip.is_none() == true {
                return Err(AnyError(format!("no ip provided or detected")))?
            } else {
                ip.unwrap()
            }
        };  

        let ip = Ip::load(ip_path)?;

        // load the ip's manifest 
        if self.units == true {
            // force computing the primary design units if a development version
            let units = Ip::collect_units(true, &ip.get_root())?;
            println!("{}", Self::format_units_table(units.into_iter().map(|(_, unit)| unit).collect()));
            return Ok(())
        }

        // display all installed versions in the cache
        if self.tags == true {
            return match catalog.get_possible_versions(ip.get_man().get_ip().get_name()) {
                Some(vers) => {
                    match vers.len() {
                        0 => { println!("info: no versions in the cache") },
                        _ => {
                            // further restrict versions if a particular version is set
                            vers.iter()
                                .filter(move |p| self.version.is_none() == true || version::is_compatible(self.version.as_ref().unwrap().as_specific().unwrap(), &p) == true)
                                .for_each(|v| {
                                    println!("{}", v);
                                });
                        }
                    }
                    Ok(())
                }
                None => Err(AnyError(format!("no ip found in catalog")))?,
            };
        }

        // print the manifest data "pretty"
        let s = toml::to_string_pretty(ip.get_man())?;
        println!("{}", s);
        Ok(())
    }
}

impl Show {
    fn run(&self) -> Result<(), Fault> {
        Ok(())
    }

    /// Creates a string for to display the primary design units for the particular ip.
    fn format_units_table(table: Vec<PrimaryUnit>) -> String {
        let header = format!("\
{:<32}{:<14}{:<9}
{:->32}{3:->14}{3:->9}\n",
"Identifier", "Type", "Public", " ");
        let mut body = String::new();

        let mut table = table;
        table.sort_by(|a, b| a.get_iden().cmp(b.get_iden()));
        for unit in table {
            body.push_str(&format!("{:<32}{:<14}{:<2}\n", 
                unit.get_iden().to_string(), 
                unit.to_string(), 
                "y"));
        }
        header + &body
    }
}

const HELP: &str = "\
Print information about an ip.

Usage:
    orbit show [options]

Options:
    --ip <name>                 the package to request data about
    --versions                  display the list of possible versions
    --ver, -v <version>         select a particular existing ip version
    --units                     display primary design units within an ip


Use 'orbit help show' to learn more about the command.
";

// FUTURE FLAGS
// ============
// --changes                   view the changelog
// --readme                    view the readme
// --range <version:version>   narrow the displayed version list