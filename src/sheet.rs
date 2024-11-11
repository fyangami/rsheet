use petgraph::{prelude::DiGraphMap, Direction};
use rsheet_lib::{
    cell_expr::{self, CellArgument},
    cell_value::CellValue,
    cells,
    command::CellIdentifier,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        mpsc::{channel, Sender},
        Arc, RwLock,
    },
    thread,
    time::SystemTime,
};

pub struct Sheet {
    cells: Arc<RwLock<HashMap<u32, HashMap<u32, Arc<RwLock<SheetCell>>>>>>,
    dep_graph: Arc<RwLock<DiGraphMap<CellIdentifier, ()>>>,
    dep_update_tx: Sender<CellIdentifier>,
}

struct SheetCell {
    value: CellValue,
    last_modify: u64,
    expr_raw: String,
}

const LIST_MATRIX_SEPARATOR: char = '_';

impl Sheet {
    pub fn new_sheet() -> Arc<Sheet> {
        let (tx, rx) = channel();

        let sht = Arc::new(Sheet {
            cells: Arc::new(RwLock::new(HashMap::new())),
            dep_update_tx: tx,
            dep_graph: Arc::new(RwLock::new(DiGraphMap::new())),
        });
        let sht_cloned = sht.clone();
        thread::spawn(move || loop {
            match rx.recv() {
                Ok(cell) => {
                    sht_cloned.update_cell_dependents(cell, current_ts());
                }
                _ => return,
            }
        });
        sht
    }

    pub fn sheet_get(&self, ident: &CellIdentifier) -> Result<CellValue, String> {
        self.get_cell_value(ident)
    }

    pub fn sheet_set_now(&self, ident: CellIdentifier, expr_raw: String) -> Result<(), String> {
        self.sheet_set(ident, expr_raw, current_ts())
    }

    pub fn sheet_set(
        &self,
        ident: CellIdentifier,
        expr_raw: String,
        last_modify: u64,
    ) -> Result<(), String> {
        let expr = cell_expr::CellExpr::new(&expr_raw);
        let mut vars = HashMap::new();
        let mut new_deps = HashSet::new();
        for name in expr.find_variable_names() {
            let (idents, arguments) = self.name_to_cell_values(&name)?;
            vars.insert(name, arguments);
            new_deps.extend(idents);
        }

        let val = expr
            .evaluate(&vars)
            .map_err(|_| "evaluate error".to_string())?;

        // update cell
        let update_dep_require = match self.get(&ident) {
            Ok(Some(cell)) => {
                // update cell
                let cell_guard = &mut cell.write().map_err(|e| e.to_string())?;
                if cell_guard.last_modify <= last_modify {
                    cell_guard.value = val;
                    cell_guard.last_modify = last_modify;
                    let update_dep_require = cell_guard.expr_raw != expr_raw;
                    cell_guard.expr_raw = expr_raw;
                    update_dep_require
                } else {
                    false
                }
            }
            Ok(None) => {
                let cell = Arc::new(RwLock::new(SheetCell {
                    value: val,
                    last_modify: last_modify,
                    expr_raw,
                }));
                self.put(&ident, cell.clone())?;
                true
            }
            Err(e) => {
                return Err(e);
            }
        };
        if update_dep_require {
            // update dep graph
            let prev_deps = self.update_dep_graph(ident.clone(), new_deps)?;
            // detect graph
            // check circular dependency
            if self.check_circular_dependency(&mut HashSet::new(), ident)? {
                // rollback dep graph
                self.update_dep_graph(ident, prev_deps)?;
                return Err("circular dependency detected".to_string());
            }
        }
        // synchronizing all refs
        self.dep_update_tx.send(ident).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn update_dep_graph(
        &self,
        ident: CellIdentifier,
        new_deps: HashSet<CellIdentifier>,
    ) -> Result<HashSet<CellIdentifier>, String> {
        let mut graph = self.dep_graph.write().map_err(|e| e.to_string())?;
        if !graph.contains_node(ident) {
            graph.add_node(ident);
        }
        let prev_deps = graph.neighbors(ident).collect::<HashSet<CellIdentifier>>();
        for ele in prev_deps.iter() {
            graph.remove_edge(ident, ele.clone());
        }
        for ele in new_deps {
            if !graph.contains_node(ele) {
                graph.add_node(ele);
            }
            graph.add_edge(ident, ele, ());
        }
        Ok(prev_deps)
    }

    fn update_cell_dependents(&self, ident: CellIdentifier, last_modify: u64) {
        let refs: HashSet<CellIdentifier> = match self.dep_graph.read() {
            Ok(graph) => graph
                .neighbors_directed(ident, Direction::Incoming)
                .collect(),
            Err(e) => {
                log::error!("update cell dependents failed: {}", e);
                return;
            }
        };
        refs.iter().for_each(|ident| match self.get(ident) {
            Ok(Some(cell)) => {
                match cell.read() {
                    Ok(cell) => {
                        let expr_raw = cell.expr_raw.to_owned();
                        drop(cell);
                        let _ = self
                            .sheet_set(ident.clone(), expr_raw, last_modify)
                            .inspect_err(|e: &String| {
                                log::error!("update cell dependents failed: {}", e)
                            });
                    }
                    Err(e) => {
                        log::error!("update cell dependents failed: {}", e)
                    }
                }
                self.update_cell_dependents(ident.clone(), last_modify);
            }
            Ok(None) => {}
            Err(e) => {
                log::error!("update cell dependents failed: {}", e)
            }
        });
    }

    fn put(&self, ident: &CellIdentifier, new_cell: Arc<RwLock<SheetCell>>) -> Result<(), String> {
        let mut cells = self.cells.write().map_err(|e| e.to_string())?;
        let col = cells.entry(ident.col).or_insert_with(HashMap::new);
        col.insert(ident.row, new_cell);
        Ok(())
    }

    fn get(&self, ident: &CellIdentifier) -> Result<Option<Arc<RwLock<SheetCell>>>, String> {
        let cells = self.cells.read().map_err(|e| e.to_string())?;
        match cells.get(&ident.col) {
            Some(col) => match col.get(&ident.row) {
                Some(cell) => Ok(Some(cell.clone())),
                _ => Ok(None),
            },
            _ => Ok(None),
        }
    }

    fn check_circular_dependency(
        &self,
        visited: &mut HashSet<CellIdentifier>,
        ident: CellIdentifier,
    ) -> Result<bool, String> {
        if visited.contains(&ident) {
            return Ok(true);
        }
        visited.insert(ident);
        let graph = self.dep_graph.read().map_err(|e| e.to_string())?;
        for dep in graph.neighbors(ident.clone()) {
            match self.check_circular_dependency(visited, dep) {
                Ok(exits) if exits => {
                    return Ok(true);
                }
                _ => {}
            }
        }
        visited.remove(&ident);
        return Ok(false);
    }

    pub fn ident_to_name(ident: &CellIdentifier) -> String {
        format!(
            "{}{}",
            cells::column_number_to_name(ident.col),
            ident.row + 1
        )
    }

    fn name_to_cell_values(
        &self,
        name: &str,
    ) -> Result<(Vec<CellIdentifier>, CellArgument), String> {
        match name.split_once(LIST_MATRIX_SEPARATOR) {
            Some((start_name, end_name)) => {
                let start_ident = start_name.parse::<CellIdentifier>()?;
                let end_ident = end_name.parse::<CellIdentifier>()?;
                let mut matrix = Vec::new();
                let mut idents = Vec::new();
                for col in start_ident.col..=end_ident.col {
                    let mut col_vector = Vec::new();
                    for row in start_ident.row..=end_ident.row {
                        let ident = CellIdentifier { col, row };
                        col_vector.push(self.get_cell_value(&ident)?);
                        idents.push(ident);
                    }
                    matrix.push(col_vector);
                }
                if matrix.len() == 0 {
                    return Err("Empty matrix".to_string());
                }
                if matrix.len() == 1 {
                    // only one col
                    return Ok((idents, CellArgument::Vector(matrix[0].to_owned())));
                }
                if matrix[0].len() == 1 {
                    // test one row
                    let mut row_vector = Vec::new();
                    for ele in matrix {
                        if ele.len() != 1 {
                            // not a list
                            break;
                        }
                        row_vector.push(ele[0].to_owned());
                    }
                    return Ok((idents, CellArgument::Vector(row_vector)));
                }
                return Ok((idents, CellArgument::Matrix(matrix)));
            }
            None => {
                let ident = name.parse::<CellIdentifier>()?;
                return Ok((
                    vec![ident],
                    CellArgument::Value(self.get_cell_value(&ident)?),
                ));
            }
        }
    }

    fn get_cell_value(&self, ident: &CellIdentifier) -> Result<CellValue, String> {
        match self.get(ident)? {
            Some(cell) => {
                let cell = cell.read().map_err(|e| e.to_string())?;
                Ok(cell.value.clone())
            }
            _ => Ok(CellValue::None),
        }
    }
}

fn current_ts() -> u64 {
    let since_the_epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();

    let timestamp_secs: u64 = since_the_epoch.as_secs();

    let timestamp_nanos: u64 = since_the_epoch.subsec_nanos() as u64;

    let timestamp_combined: u64 = timestamp_secs * 1_000_000_000 + timestamp_nanos;
    return timestamp_combined;
}
