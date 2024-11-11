use rsheet_lib::{
    cell_expr::{self, CellArgument},
    cell_value::CellValue,
    cells,
    command::CellIdentifier,
    replies::Reply,
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
    dep_update_tx: Sender<CellIdentifier>,
}

struct SheetCell {
    value: CellValue,
    deps: HashSet<CellIdentifier>,
    refs: HashSet<CellIdentifier>,
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
        });
        let sht_cloned = sht.clone();
        thread::spawn(move || loop {
            match rx.recv() {
                Ok(ident) => {
                    sht_cloned.sheet_set(&ident, "".to_string(), current_ts());
                }
                _ => return,
            }
        });
        sht
    }

    pub fn sheet_get(&self, ident: &CellIdentifier) -> Result<CellValue, String> {
        self.get_cell_value(ident)
    }

    pub fn sheet_set_now(&self, ident: &CellIdentifier, expr_raw: String) -> Result<(), String> {
        self.sheet_set(ident, expr_raw, current_ts())
    }

    pub fn sheet_set(
        &self,
        ident: &CellIdentifier,
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
        let cell = match self.get(ident) {
            Ok(Some(cell)) => {
                // update cell
                let cell_guard = &mut cell.write().map_err(|e| e.to_string())?;
                for ele in cell_guard.deps.iter() {
                    match self.get(ele)? {
                        Some(dep_cell) => {
                            let dep_cell_guard =
                                &mut dep_cell.write().map_err(|e| e.to_string())?;
                            dep_cell_guard.refs.remove(ident);
                        }
                        _ => {}
                    }
                }
                cell_guard.deps = new_deps;
                if cell_guard.last_modify <= last_modify {
                    cell_guard.value = val;
                    cell_guard.last_modify = last_modify;
                }
                cell_guard.expr_raw = expr_raw;
                cell.clone()
            }
            Ok(None) => {
                let cell = Arc::new(RwLock::new(SheetCell {
                    value: val,
                    deps: new_deps,
                    refs: HashSet::new(),
                    last_modify: last_modify,
                    expr_raw,
                }));
                self.put(ident, cell.clone())?;
                cell
            }
            Err(e) => {
                return Err(e);
            }
        };
        // renew deps links
        {
            let cell_guard = &mut cell.write().map_err(|e| e.to_string())?;

            for ele in cell_guard.deps.iter() {
                match self.get(&ele)? {
                    Some(dep_cell) => {
                        let dep_cell_guard = &mut dep_cell.write().map_err(|e| e.to_string())?;
                        dep_cell_guard.refs.insert(ident.clone());
                    }
                    _ => return Err("dep cell not found".to_string()),
                }
            }
        }
        // check circular dependency
        if self.check_circular_dependency(&mut HashSet::new(), cell.clone(), &ident)? {
            return Err("circular dependency detected".to_string());
        }

        // synchronizing all refs
        self.dep_update_tx.send(ident.to_owned());
        Ok(())
    }

    fn update_cell_dependents(&self, cell: Arc<RwLock<SheetCell>>, last_modify: u64) {
        match &cell.read() {
            Ok(cell) => {
                cell.refs.iter().for_each(|ident| match self.get(ident) {
                    Ok(Some(cell)) => {
                        match cell.read() {
                            Ok(cell) => {
                                self.sheet_set(ident, cell.expr_raw.to_owned(), last_modify);
                            }
                            _ => {}
                        }
                        self.update_cell_dependents(cell, last_modify);
                    }
                    _ => {}
                });
            }
            _ => return,
        }
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

    pub fn check_circular_dependency(
        &self,
        visited: &mut HashSet<CellIdentifier>,
        cell: Arc<RwLock<SheetCell>>,
        ident: &CellIdentifier,
    ) -> Result<bool, String> {
        let cell = cell.read().map_err(|e| e.to_string())?;
        if visited.contains(ident) {
            return Ok(true);
        }
        visited.insert(ident.to_owned());
        for dep in cell.deps.iter() {
            let exits = match self.get(&dep)? {
                Some(dep_cell) => self.check_circular_dependency(visited, dep_cell, dep)?,
                _ => false,
            };
            if exits {
                return Ok(true);
            }
        }
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
    // 获取秒数并转换为u64
    let timestamp_secs: u64 = since_the_epoch.as_secs();

    // 获取纳秒部分并转换为u64
    let timestamp_nanos: u64 = since_the_epoch.subsec_nanos() as u64;

    // 组合秒数和纳秒部分（如果需要精确到纳秒的时间戳）
    let timestamp_combined: u64 = timestamp_secs * 1_000_000_000 + timestamp_nanos;
    return timestamp_combined;
}
