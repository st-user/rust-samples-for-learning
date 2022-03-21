use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct Employee {
    pub id: i64,
    pub name: String,
    pub role: Role,
}

impl Employee {
    pub fn from(another: &Employee) -> Employee {
        Employee {
            id: another.id.clone(),
            name: another.name.clone(),
            role: another.role.clone(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub enum Role {
    SuperAdmin,
    Admin,
    Agent,
}

impl Clone for Role {
    fn clone(&self) -> Role {
        match self {
            Role::SuperAdmin => Role::SuperAdmin,
            Role::Admin => Role::Admin,
            Role::Agent => Role::Agent,
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct EmployeeBody {
    pub name: String,
    pub role: Role,
}

#[derive(Serialize)]
pub struct ErrorMessage {
    pub code: u16,
    pub message: String,
}
