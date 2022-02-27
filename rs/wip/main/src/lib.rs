pub mod synth;
use synth::makenode;

use std::collections::HashMap;
use petgraph::{graph::NodeIndex, stable_graph::StableDiGraph};
use dasp_graph::{NodeData, BoxedNodeSend, Processor, node::Sum, Buffer }; //Input, NodeBuffer

use glicol_parser::*; 
use pest::iterators::Pair;
use lcs_diff::{diff, DiffResult};

pub type GlicolNodeData<const N: usize> = NodeData<BoxedNodeSend<N>, N>;
pub type GlicolGraph<const N: usize> = StableDiGraph<GlicolNodeData<N>, (), u32>;
pub type GlicolProcessor<const N: usize> = Processor<GlicolGraph<N>, N>;

pub struct Engine<'a, const N: usize> {
    pub graph: GlicolGraph<N>,
    pub processor: GlicolProcessor<N>,
    code: &'static str,
    ast: HashMap<&'a str, (Vec<&'a str>, Vec<Pair<'a, Rule>>)>,
    new_ast: HashMap<&'a str, (Vec<&'a str>, Vec<Pair<'a, Rule>>)>,
    pub index_info: HashMap<&'a str, Vec<NodeIndex>>,
    output_index: NodeIndex,
    node_add_list: Vec<(&'a str, usize, GlicolNodeData<N>)>,
    node_remove_list: Vec<(&'a str, usize)>,
    node_update_list: Vec<(&'a str, usize, Pair<'a, Rule>)>,    
}

impl<const N: usize> Engine<'static, N> {
    pub fn new() -> Self {
        let mut graph = GlicolGraph::<N>::with_capacity(1024, 1024);
        let output_index = graph.add_node(NodeData::new2(BoxedNodeSend::<N>::new(Sum{})));
        Self {
            graph,
            processor: GlicolProcessor::<N>::with_capacity(1024),
            ast: HashMap::new(),
            new_ast: HashMap::new(),
            code: "",
            index_info: HashMap::new(),
            output_index,
            node_add_list: vec![],
            node_remove_list: vec![],
            node_update_list: vec![],
        }
    }

    pub fn send_msg(
        &mut self, 
        chain_name: &str, 
        node_index_in_chain: u8, 
        msg: (u8, &str)
    ) {
        let index = self.index_info[chain_name][node_index_in_chain as usize];
        self.graph[index].node.send_msg(msg);
    }

    // todo pub fn set bpm set sr set seed ...

    pub fn set_code(&mut self, code: &'static str) {
        self.code = code;
    }

    pub fn update(&mut self) {
        self.parse();
        self.make_graph();
    }

    // prepare the NodeData::new2(BoxedNodeSend::<N>::new(Sum{}))
    // but do not do anything to the graph
    // get: add info , which chain, where
    // modify info 
    // delete info
    // sidechain info, when handling the graph, check if all the sidechain exists
    pub fn parse(&mut self) {
        self.new_ast = get_glicol_ast(&self.code).unwrap();
        self.node_add_list.clear();
        self.node_update_list.clear();
        self.node_remove_list.clear();
        // also remove the whole chain in_old but not_in_new, after ensuring there is no problem with new stuff
        // println!("\n\nold ast {:?}\n\n new {:?}", self.ast, self.new_ast);
        for (key, node_info_tuple) in &self.new_ast {
            if self.ast.contains_key(key) {
                let old_chain = &self.ast[key].0;
                let new_chain = &node_info_tuple.0;
                let old_chain_para = &self.ast[key].1;
                let new_chain_para = &node_info_tuple.1;
                for action in diff(old_chain, new_chain) {
                    match action {
                        DiffResult::Common(v) => {
                            // let common_node_name = v.data;
                            let old_i = v.old_index.unwrap();
                            let new_i = v.new_index.unwrap();
                            println!("common {:?}", v);
                            println!("common node: old_index {:?}", old_i);
                            // println!("common para {:?}", old_chain_para[old_i]);
                            // println!("new para {:?}", new_chain_para[new_i]);
                            self.node_update_list.push(
                                (key, // which chain
                                old_i, // where in chain
                                new_chain_para[new_i].clone() // new paras
                            ))
                        },
                        DiffResult::Removed(v) => {
                            // let removed_node_name = v.data;
                            let old_i = v.old_index.unwrap();
                            self.node_remove_list.push((key, old_i));
                            println!("Removed {:?}", v)
                        },
                        DiffResult::Added(v) => {
                            println!("Added {:?}", v);
                            let new_i = v.new_index.unwrap();
                            let insert_i = v.new_index.unwrap();
                            let nodename = v.data;
                            let mut paras = new_chain_para[new_i].clone();
                            let nodedata = makenode(nodename, &mut paras);
                            self.node_add_list.push((key, insert_i, nodedata))
                        },
                    }
                }
                // println!("diff {:?}", diff(old_chain, new_chain));
            } else {
                for i in 0..node_info_tuple.0.len() {
                    let name = node_info_tuple.0[i];
                    let mut paras = node_info_tuple.1[i].clone();
                    let nodedata = makenode(name, &mut paras);
                    self.node_add_list.push((key, i, nodedata));
                };
                // self.ast.insert(key, node_info_tuple.clone());
                // self.add_whole_chain(key, node_info_tuple.clone());
            }
        }
    }

    pub fn make_graph(&mut self) {
        self.handle_ref_check();
        self.handle_remove_chain();
        self.handle_node_remove();
        self.handle_node_add();
        self.handle_node_update();
        self.handle_connection();
        self.ast = self.new_ast.clone();
    }

    pub fn handle_ref_check(&self) {
        // ref pair is like (~mod -> a node [e.g key: out, pos_in_chain: 3])
        // ref check should use the new ast hashmap
        // because old ast hashmap has something that may need to be deleted
    }

    pub fn handle_remove_chain(&mut self) {
        // there are some chains show up in old_ast but not in new ast
        for key in self.ast.keys() {
            if !self.new_ast.contains_key(key) {
                println!("remove {:?}", key);
                for index in &self.index_info[key] {
                    self.graph.remove_node(*index);
                }
                self.index_info.remove_entry(key);       
            }
        }
    }

    pub fn handle_node_add(&mut self) {
        while !self.node_add_list.is_empty() {
            let (key, position_in_chain, nodedata) = self.node_add_list.remove(0);
            if !self.index_info.contains_key(key) {
                self.index_info.insert(key, vec![]);
            };
            let nodeindex = self.graph.add_node(nodedata);
            if let Some(chain) = self.index_info.get_mut(key) {
                chain.insert(position_in_chain, nodeindex);
            }
        }
    }
    pub fn handle_node_update(&mut self) {
        while !self.node_update_list.is_empty() {
            let (key, position_in_chain, paras) = self.node_update_list.remove(0);
            if let Some(chain) = self.index_info.get_mut(key) {
                self.graph[chain[position_in_chain]].node.send_msg((0, paras.as_str()));
            }
        }
    }
    pub fn handle_node_remove(&mut self) {
        while !self.node_remove_list.is_empty() {
            let (key, position_in_chain) = self.node_remove_list.remove(0);
            if let Some(chain) = self.index_info.get_mut(key) {
                let node_index = chain[position_in_chain];
                self.graph.remove_node(node_index);
                chain.remove(position_in_chain);
            }
        }
    }

    pub fn handle_connection(&mut self) {
        self.graph.clear_edges();
        for (key, chain) in &self.index_info {
            match chain.len() {
                0 => {},
                1 => {
                    self.graph.add_edge(chain[0], self.output_index, ());
                },
                2 => {
                    self.graph.add_edge(chain[0], chain[1], ());
                    self.graph.add_edge(chain[1], self.output_index, ());
                },
                _ => {
                    for i in 0..chain.len() - 1 {
                        if i == chain.len() - 1 {
                            self.graph.add_edge(chain[i], self.output_index ,());
                        } else {
                            self.graph.add_edge(chain[i],chain[i+1] ,());
                        }
                    }
                }
            }
        }
    }

    pub fn next_block(&mut self) -> &[Buffer<N>] {  //  -> &Vec<Buffer<N>> 
        self.processor.process(&mut self.graph, self.output_index);
        // println!("result {:?}", &self.graph[self.output_index].buffers);
        &self.graph[self.output_index].buffers
    }
}