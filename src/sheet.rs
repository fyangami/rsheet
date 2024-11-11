use rsheet_lib::{
    cell_expr::{self, CellArgument},
    cell_value::CellValue,
    cells,
    command::CellIdentifier,
    replies::Reply,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
    time::SystemTime,
};

pub struct Sheet {
    cells: Arc<RwLock<HashMap<u32, HashMap<u32, Arc<RwLock<SheetCell>>>>>>,
}

struct SheetCell {
    value: CellValue,
    deps: HashSet<String>,
    refs: HashSet<String>,
    last_modify: u64,
    expr_raw: String,
}

const LIST_MATRIX_SEPARATOR: char = '_';

impl Sheet {
    pub fn new() -> Sheet {
        Sheet {
            cells: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn sheet_get(&self, ident: &CellIdentifier) -> Result<CellValue, String> {
        self.get_cell_value(ident)
    }

    pub fn sheet_set(&mut self, ident: &CellIdentifier, expr_raw: String) -> Result<(), String> {
        let expr = cell_expr::CellExpr::new(&expr_raw);
        let mut vars = HashMap::new();
        let mut new_deps = HashSet::new();
        for name in expr.find_variable_names() {
            let (idents, arguments) = self.name_to_cell_values(&name)?;
            vars.insert(name, arguments);
        }
        let val = expr
            .evaluate(&vars)
            .map_err(|_| "evaluate error".to_string())?;
        let ts = current_ts();
        // update cell
        let cell = match self.get(ident) {
            Ok(Some(cell)) => {
                // update cell
                let mut cell_guard = cell.write().map_err(|e| e.to_string())?;
                cell
            }
            Ok(None) => {
                let cell = Arc::new(RwLock::new(SheetCell {
                    value: val,
                    deps: new_deps,
                    refs: HashSet::new(),
                    last_modify: ts,
                    expr_raw,
                }));
                self.put(ident, cell.clone())?;
                cell
            }
            Err(e) => {
                return Err(e);
            }
        };
        if self.check_circular_dependency(
            &mut HashSet::new(),
            Self::ident_to_name(ident),
            &new_deps,
        ) {
            return Reply::Error("circular dependency detected".to_string());
        }
        // let reply = match expr.evaluate(&vars) {
        //     Ok(value) => {
        //         self.set_cell_value(ident, value.clone());
        //         Reply::Value(Self::ident_to_name(ident), value)
        //     }
        //     Err(_) => Reply::Error("Value dependent error value".to_string()),
        // };
        // return reply;
        todo!()
    }

    fn update_cell_dependents(cell: Arc<RwLock<SheetCell>>, new_deps: HashSet<String>) {}

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

    fn update_deps(&mut self, ident: &CellIdentifier, new_deps: &HashSet<String>) {
        // TODO circular dep
        let ident_name = Self::ident_to_name(ident);
        // handle prev cell deps
        self.dep_map
            .entry(ident_name.clone())
            .or_insert_with(HashSet::new);
        let cell_deps = self.dep_map.get(&ident_name).expect("created above");
        cell_deps.iter().for_each(|dep| {
            let refs = self
                .ref_map
                .entry(dep.to_owned())
                .or_insert_with(HashSet::new);
            refs.remove(dep);
        });
        let cell_deps = self.dep_map.get_mut(&ident_name).expect("created above");
        // handle new cell deps
        *cell_deps = new_deps.to_owned();
        cell_deps.iter().for_each(|dep| {
            self.ref_map
                .entry(dep.to_owned())
                .or_insert_with(HashSet::new);
            let refs = self.ref_map.get_mut(dep).expect("created above");
            refs.insert(ident_name.to_owned());
        });
    }

    pub fn check_circular_dependency(
        &self,
        visited: &mut HashSet<String>,
        cell_name: String,
        cell_deps: &HashSet<String>,
    ) -> bool {
        if visited.contains(&cell_name) {
            return true;
        }
        visited.insert(cell_name);
        for dep in cell_deps {
            let exits = match self.dep_map.get(dep) {
                Some(sub_deps) => self.check_circular_dependency(visited, dep.to_owned(), sub_deps),
                _ => false,
            };
            if exits {
                return true;
            }
        }
        return false;
    }

    pub fn update_all_dependencies(sht: Arc<RwLock<Sheet>>, ident: &CellIdentifier) {
        // TODO
        let ident_name = Self::ident_to_name(ident);
    }

    fn ident_to_name(ident: &CellIdentifier) -> String {
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
    // 获取秒数并转换为u64
    let timestamp_secs: u64 = since_the_epoch.as_secs();

    // 获取纳秒部分并转换为u64
    let timestamp_nanos: u64 = since_the_epoch.subsec_nanos() as u64;

    // 组合秒数和纳秒部分（如果需要精确到纳秒的时间戳）
    let timestamp_combined: u64 = timestamp_secs * 1_000_000_000 + timestamp_nanos;
    return timestamp_combined;
}
