use rsheet_lib::{
    cell_expr::{self, CellArgument},
    cell_value::CellValue,
    cells,
    command::CellIdentifier,
    replies::Reply,
};
use std::{collections::{HashMap, HashSet}, sync::{Arc, Mutex}};

pub struct Sheet {
    cells: HashMap<u32, HashMap<u32, CellValue>>,
    dep_map: HashMap<String, HashSet<String>>,
    ref_map: HashMap<String, HashSet<String>>,
}

const LIST_MATRIX_SEPARATOR: char = '_';

impl Sheet {
    pub fn new() -> Sheet {
        Sheet {
            cells: HashMap::new(),
            dep_map: HashMap::new(),
            ref_map: HashMap::new(),
        }
    }

    pub fn set(&mut self, ident: &CellIdentifier, expr: String) -> Reply {
        let expr = cell_expr::CellExpr::new(&expr);
        let mut vars = HashMap::new();
        let mut new_deps = HashSet::new();
        for name in expr.find_variable_names() {
            match self.name_to_cell_values(&name) {
                Ok((idents, argument)) => {
                    vars.insert(name, argument);
                    new_deps.extend(idents.iter().map(Self::ident_to_name));
                }
                Err(err) => {
                    return Reply::Error(err);
                }
            }
        }
        // update all deps
        self.update_deps(ident, new_deps);
        let reply = match expr.evaluate(&vars) {
            Ok(value) => {
                self.set_cell_value(ident, value.clone());
                Reply::Value(Self::ident_to_name(ident), value)
            }
            Err(_) => Reply::Error("Value dependent error value".to_string()),
        };
        return reply;
    }

    fn update_deps(&mut self, ident: &CellIdentifier, new_deps: HashSet<String>) {
        // TODO circular dep
        let ident_name = Self::ident_to_name(ident);
        // handle prev cell deps
        self.dep_map
            .entry(ident_name.clone())
            .or_insert_with(HashSet::new);
        let cell_deps = self.dep_map.get(&ident_name).expect("created above");
        cell_deps.iter().for_each(|dep| {
            let refs = self.ref_map.entry(dep.to_owned()).or_insert_with(HashSet::new);
            refs.remove(dep);
        });
        let cell_deps = self.dep_map.get_mut(&ident_name).expect("created above");
        // handle new cell deps
        *cell_deps = new_deps;
        cell_deps.iter().for_each(|dep| {
            self.ref_map
                .entry(dep.to_owned())
                .or_insert_with(HashSet::new);
            let refs = self.ref_map.get_mut(dep).expect("created above");
            refs.insert(ident_name.to_owned());
        });
    }
    
    pub fn update_all_dependencies(_sht: Arc<Mutex<Sheet>>, ident: &CellIdentifier) {
        // TODO
        let _ident_name = Self::ident_to_name(ident);
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
                        col_vector.push(self.get_cell_value(&ident));
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
                // TODO: handle single cell
                let ident = name.parse::<CellIdentifier>()?;
                return Ok((
                    vec![ident],
                    CellArgument::Value(self.get_cell_value(&ident)),
                ));
            }
        }
    }

    pub fn get(&self, ident: &CellIdentifier) -> Reply {
        return Reply::Value(Self::ident_to_name(ident), self.get_cell_value(ident));
    }

    fn get_cell_value(&self, ident: &CellIdentifier) -> CellValue {
        match self.cells.get(&ident.col) {
            Some(col) => match col.get(&ident.row) {
                Some(cell) => cell.clone(),
                None => CellValue::None,
            },
            _ => CellValue::None,
        }
    }

    fn set_cell_value(&mut self, ident: &CellIdentifier, value: CellValue) {
        self.cells
            .entry(ident.col)
            .or_insert_with(HashMap::new)
            .insert(ident.row, value);
    }
}
