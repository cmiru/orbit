use crate::Command;
use crate::FromCli;
use crate::interface::cli::Cli;
use crate::interface::arg::Optional;
use crate::interface::errors::CliError;
use crate::core::context::Context;
use std::ffi::OsString;
use std::io::Write;
use crate::core::fileset::Fileset;

#[derive(Debug, PartialEq)]
pub struct Plan {
    plugin: Option<String>,
    bench: Option<Identifier>,
    top: Option<Identifier>,
    build_dir: Option<String>,
    filesets: Option<Vec<Fileset>>
}

impl Command for Plan {
    type Err = Box<dyn std::error::Error>;
    fn exec(&self, c: &Context) -> Result<(), Self::Err> {
        // check that user is in an IP directory
        c.goto_ip_path()?;
        // set top-level environment variables (@TODO verify these are valid toplevels to be set!)
        if let Some(t) = &self.top {
            std::env::set_var("ORBIT_TOP", t.to_string());
        }
        if let Some(b) = &self.bench {
            std::env::set_var("ORBIT_BENCH", b.to_string());
        }
        // determine the build directory
        let b_dir = if let Some(dir) = &self.build_dir {
            dir
        } else {
            c.get_build_dir()
        };
        // find plugin filesets
        let plug_fset = if let Some(plug) = &self.plugin {
            Some(c.get_plugins().get(plug).expect(&format!("plugin {} does not exist", plug)).filesets())
        } else {
            None
        };
        // @TODO pass in the current IP struct
        Ok(self.run(b_dir, plug_fset))
    }
}

use crate::core::vhdl::parser;
use crate::util::graph::Graph;
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
struct HashNode {
    index: usize,
    entity: parser::Entity,
    files: Vec<String>,
}

impl HashNode {
    fn index(&self) -> usize {
        self.index
    }
    
    fn new(entity: parser::Entity, index: usize, file: String) -> Self {
        let mut set = Vec::new();
        set.push(file);
        Self {
            entity: entity,
            index: index,
            files: set,
        }
    }

    fn add_file(&mut self, file: String) {
        if self.files.contains(&file) == false {
            self.files.push(file);
        }
    }
}

use crate::core::vhdl::vhdl::Identifier;

impl Plan {
    fn run(&self, build_dir: &str, plug_filesets: Option<&Vec<Fileset>>) -> () {
        let mut build_path = std::env::current_dir().unwrap();
        build_path.push(build_dir);
        // gather filesets
        let files = crate::core::fileset::gather_current_files(&std::env::current_dir().unwrap());

        // @TODO refactor graph and hold onto entity structs rather than just their identifier
        let mut g = Graph::new();
        // entity identifier, HashNode
        let mut map = HashMap::<Identifier, HashNode>::new();
        // store map key at the node index @TODO move into the edge data in graph
        let mut inverse_map = Vec::<Identifier>::new();

        let mut archs: Vec<(parser::Architecture, String)> = Vec::new();
        // read all files
        for source_file in &files {
            if crate::core::fileset::is_vhdl(&source_file) == true {
                let contents = std::fs::read_to_string(&source_file).unwrap();
                let symbols = parser::VHDLParser::read(&contents).into_symbols();
                // add all entities to a graph and store architectures for later analysis
                let mut iter = symbols.into_iter().filter_map(|f| {
                    match f {
                        parser::VHDLSymbol::Entity(e) => Some(e),
                        parser::VHDLSymbol::Architecture(arch) => {
                            archs.push((arch, source_file.to_string()));
                            None
                        }
                        _ => None,
                    }
                });
                while let Some(e) = iter.next() {
                    let index = g.add_node();
                    inverse_map.push(e.get_name().clone());
                    map.insert(e.get_name().clone(), HashNode::new(e, index, source_file.to_string()));
                }
            }
        }

        // go through all architectures and make the connections
        let mut archs = archs.into_iter();
        while let Some((arch, file)) = archs.next() {
            // link to the owner and add architecture's source file
            let entity_node = map.get_mut(&arch.entity()).unwrap();
            entity_node.add_file(file);
            // create edges
            for dep in arch.edges() {
                // verify the dep exists
                if let Some(node) = map.get(dep) {
                    g.add_edge(node.index(), map.get(arch.entity()).unwrap().index());
                }
            }
        }

        // sort
        let order = g.topological_sort();
        println!("{:?}", order);
        println!("{:?}", map);

        let mut bench = if let Some(t) = &self.bench {
            match map.get(&t) {
                Some(node) => {
                    if node.entity.is_testbench() == false {
                        panic!("entity {} is not a testbench and cannot be bench; please use --top", t)
                    }
                    node.index()
                },
                None => panic!("no entity named {}", t)
            }
        } else if self.top.is_none() {
            // filter to display tops that have ports (not testbenches)
            g.find_root().expect("multiple testbenchs (or zero) are possible")
        } else {
            0 // still could possibly be found by top level is top is some
        };

        // determine the top-level node index
        let top = if let Some(t) = &self.top {
            match map.get(&t) {
                Some(node) => {
                    if node.entity.is_testbench() == true {
                        panic!("entity {} is a testbench and cannot be top; please use --bench", t)
                    }
                    let n = node.index();
                    // try to detect top level testbench
                    if self.bench.is_none() {
                        // find if there is 1 successor for top
                        if g.out_degree(n) == 1 {
                            bench = g.successors(n).next().unwrap();
                        } else {
                            panic!("multiple testbenches detected for {}", node.entity.get_name())
                        }
                    }
                    n
                },
                None => panic!("no entity named {}", t)
            }
        } else {
            Self::detect_top(&g, Some(bench))
        };
        // enable immutability
        let bench = bench;

        // @TODO detect if there is a single existing testbench for the top

        let top_name = &inverse_map[top];
        let bench_name = &inverse_map[bench];

        std::env::set_var("ORBIT_TOP", &top_name.to_string());
        std::env::set_var("ORBIT_BENCH", &bench_name.to_string());
        
        // compute minimal topological ordering
        let min_order = g.minimal_topological_sort(bench);

        let mut file_order = Vec::new();
        for i in &min_order {
            // access the node key
            let key = &inverse_map[*i];
            // access the files associated with this key
            let mut v: Vec<&String> = map.get(key).as_ref().unwrap().files.iter().collect();
            file_order.append(&mut v);
        }

        // store data in blueprint TSV format
        let mut blueprint_data = String::new();

        // use command-line set filesets
        if let Some(fsets) = &self.filesets {
            for fset in fsets {
                let data = fset.collect_files(&files);
                for f in data {
                    blueprint_data += &format!("{}\t{}\t{}\n", fset.get_name(), std::path::PathBuf::from(f).file_stem().unwrap_or(&OsString::new()).to_str().unwrap(), f);
                }
            }
        }

        // collect data for the given plugin
        if let Some(fsets) = plug_filesets {
            // define pattern matching settings
            let match_opts = glob::MatchOptions {
                case_sensitive: false,
                require_literal_separator: false,
                require_literal_leading_dot: false,
            };
            // iterate through every collected file
            for file in &files {
                // check against every defined fileset for the plugin
                for fset in fsets {
                    if fset.get_pattern().matches_with(file, match_opts) == true {
                        // add to blueprint
                        blueprint_data += &fset.to_blueprint_string(file);
                    }
                }
            }
        }

        for file in file_order {
            if crate::core::fileset::is_rtl(&file) == true {
                blueprint_data += &format!("VHDL-RTL\twork\t{}\n", file);
            } else {
                blueprint_data += &format!("VHDL-SIM\twork\t{}\n", file);
            }
        }

        // create a output build directorie(s) if they do not exist
        if std::path::PathBuf::from(build_dir).exists() == false {
            std::fs::create_dir_all(build_dir).expect("could not create build dir");
        }
        // create the blueprint file
        let blueprint_path = build_path.join("blueprint.tsv");
        let mut blueprint_file = std::fs::File::create(&blueprint_path).expect("could not create blueprint.tsv file");
        // write the data
        blueprint_file.write_all(blueprint_data.as_bytes()).expect("failed to write data to blueprint");
        
        // create environment variables to .env file
        let env_path = build_path.join(".env");
        let mut env_file = std::fs::File::create(&env_path).expect("could not create .env file");
        let contents = format!("ORBIT_TOP={}\nORBIT_BENCH={}\n", &top_name, &bench_name);
        // write the data
        env_file.write_all(contents.as_bytes()).expect("failed to write data to .env file");

        // create a blueprint file
        println!("info: Blueprint created at: {}", blueprint_path.display());
    }

    /// Given a `graph` and optionally a `bench`, detect the index corresponding
    /// to the top.
    /// 
    /// This function looks and checks if there is a single predecessor to the
    /// `bench` node.
    fn detect_top(graph: &Graph, bench: Option<usize>) -> usize {
        if let Some(b) = bench {
            match graph.in_degree(b) {
                0 => panic!("no entities are tested in the testbench"),
                1 => graph.predecessors(b).next().unwrap(),
                _ => panic!("multiple tops are detected from testbench")
            }
        } else {
            todo!("find toplevel node that is not a bench")
        }
    }
}

impl FromCli for Plan {
    fn from_cli<'c>(cli: &'c mut Cli) -> Result<Self,  CliError<'c>> {
        cli.set_help(HELP);
        let command = Ok(Plan {
            top: cli.check_option(Optional::new("top").value("unit"))?,
            bench: cli.check_option(Optional::new("bench").value("tb"))?,
            plugin: cli.check_option(Optional::new("plugin"))?,
            build_dir: cli.check_option(Optional::new("build-dir").value("dir"))?,
            filesets: cli.check_option_all(Optional::new("fileset").value("key=glob"))?,
        });
        command
    }
}

const HELP: &str = "\
Generates a blueprint file.

Usage:
    orbit plan [options]              

Options:
    --top <unit>            override auto-detected toplevel entity
    --bench <tb>            override auto-detected toplevel testbench
    --plugin <plugin>       collect filesets defined for a plugin
    --build-dir <dir>       set the output build directory
    --fileset <key=glob>... set an additional fileset
    --all                   include all found HDL files

Use 'orbit help plan' to learn more about the command.
";