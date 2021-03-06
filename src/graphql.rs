use crate::{models, DbCon};
use diesel::{pg::PgConnection, prelude::*};
use juniper::{Executor, FieldResult, ID};
use juniper_eager_loading::{prelude::*, *};
use juniper_eager_loading::{EagerLoadAllChildren, GraphqlNodeForModel};
use juniper_from_schema::graphql_schema_from_file;
use rocket::{
    request::{self, FromRequest, Request},
    Outcome,
};

graphql_schema_from_file!("schema.graphql");

pub struct Context {
    db_con: DbCon,
}

impl juniper::Context for Context {}

impl<'a, 'r> FromRequest<'a, 'r> for Context {
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> request::Outcome<Context, ()> {
        let db_con = request.guard::<DbCon>()?;
        Outcome::Success(Context { db_con })
    }
}

impl Context {
    pub fn db(&self) -> &PgConnection {
        &self.db_con.0
    }
}

pub struct Query;

impl QueryFields for Query {
    fn field_users(
        &self,
        executor: &Executor<'_, Context>,
        trail: &QueryTrail<'_, User, Walked>,
    ) -> FieldResult<Vec<User>> {
        use crate::schema::users;
        let ctx = &executor.context();
        let con = &ctx.db();

        let user_models = users::table.load::<models::User>(*con)?;
        let users = map_models_to_graphql_nodes(&user_models, &trail, ctx)?;

        Ok(users)
    }

    fn field_user_connections(
        &self,
        executor: &Executor<'_, Context>,
        trail: &QueryTrail<'_, UserConnection, Walked>,
        after: Option<Cursor>,
        first: i32,
    ) -> FieldResult<UserConnection> {
        let ctx = &executor.context();
        let user_connection = user_connections(after, first, trail, ctx)?;
        Ok(user_connection)
    }
}

fn user_connections(
    cursor: Option<Cursor>,
    page_size: i32,
    trail: &QueryTrail<'_, UserConnection, Walked>,
    ctx: &Context,
) -> QueryResult<UserConnection> {
    use crate::{models::pagination::*, schema::users};

    let con = &ctx.db();

    let page_size = i64::from(page_size);

    let page_number = cursor
        .unwrap_or_else(|| Cursor("1".to_string()))
        .0
        .parse::<i64>()
        .expect("invalid cursor");
    let next_page_cursor = Cursor((page_number + 1).to_string());

    let base_query = users::table.select(users::all_columns).order(users::id);

    let (user_models, total_count) = base_query
        .paginate(page_number)
        .per_page(page_size)
        .load_and_count_pages::<models::User>(con)?;

    let users = if let Some(user_trail) = trail.edges().node().walk() {
        map_models_to_graphql_nodes(&user_models, &user_trail, ctx)?
    } else {
        vec![]
    };

    let edges = users
        .into_iter()
        .map(|user| Edge {
            node: user,
            cursor: next_page_cursor.clone(),
        })
        .collect::<Vec<_>>();

    let page_info = PageInfo {
        start_cursor: edges.first().map(|edge| edge.cursor.clone()),
        end_cursor: edges.last().map(|edge| edge.cursor.clone()),
        has_next_page: {
            let next_page = base_query
                .paginate(page_number + 1)
                .per_page(1)
                .load::<(models::User, i64)>(con)?;
            !next_page.is_empty()
        },
    };

    Ok(UserConnection {
        edges,
        page_info,
        total_count: total_count as i32,
    })
}

fn map_models_to_graphql_nodes<'a, T, M: Clone>(
    models: &[M],
    trail: &QueryTrail<'a, T, Walked>,
    ctx: &Context,
) -> Result<Vec<T>, diesel::result::Error>
where
    T: EagerLoadAllChildren
        + GraphqlNodeForModel<Model = M, Context = Context, Error = diesel::result::Error>,
{
    let mut users = T::from_db_models(models);
    T::eager_load_all_children_for_each(&mut users, models, ctx, trail)?;
    Ok(users)
}

pub struct Mutation;

impl MutationFields for Mutation {
    fn field_noop(&self, _executor: &Executor<'_, Context>) -> FieldResult<&bool> {
        Ok(&true)
    }
}

#[derive(Clone, Debug, EagerLoading)]
#[eager_loading(
    model = models::User,
    error = diesel::result::Error,
    context = Context
)]
pub struct User {
    user: models::User,
    #[has_one(default)]
    country: HasOne<Country>,
}

#[derive(Clone, Debug, EagerLoading)]
#[eager_loading(
    model = models::Country,
    error = diesel::result::Error,
    context = Context
)]
pub struct Country {
    country: models::Country,
}

impl UserFields for User {
    fn field_id(&self, _: &Executor<'_, Context>) -> FieldResult<ID> {
        Ok(ID::new(self.user.id.to_string()))
    }

    fn field_name(&self, _: &Executor<'_, Context>) -> FieldResult<&String> {
        Ok(&self.user.name)
    }

    fn field_country(
        &self,
        _: &Executor<'_, Context>,
        _trail: &QueryTrail<'_, Country, Walked>,
    ) -> FieldResult<&Country> {
        Ok(self.country.try_unwrap()?)
    }
}

impl CountryFields for Country {
    fn field_id(&self, _executor: &Executor<'_, Context>) -> FieldResult<ID> {
        Ok(ID::new(format!("{}", self.country.id)))
    }

    fn field_name(&self, _executor: &Executor<'_, Context>) -> FieldResult<&String> {
        Ok(&self.country.name)
    }
}

pub struct PageInfo {
    start_cursor: Option<Cursor>,
    end_cursor: Option<Cursor>,
    has_next_page: bool,
}

impl PageInfoFields for PageInfo {
    fn field_start_cursor(&self, _: &Executor<'_, Context>) -> FieldResult<&Option<Cursor>> {
        Ok(&self.start_cursor)
    }

    fn field_end_cursor(&self, _: &Executor<'_, Context>) -> FieldResult<&Option<Cursor>> {
        Ok(&self.end_cursor)
    }

    fn field_has_next_page(&self, _: &Executor<'_, Context>) -> FieldResult<&bool> {
        Ok(&self.has_next_page)
    }
}

pub struct UserConnection {
    edges: Vec<UserEdge>,
    page_info: PageInfo,
    total_count: i32,
}

impl UserConnectionFields for UserConnection {
    fn field_edges(
        &self,
        _: &Executor<'_, Context>,
        _: &QueryTrail<'_, UserEdge, Walked>,
    ) -> FieldResult<&Vec<UserEdge>> {
        Ok(&self.edges)
    }

    fn field_page_info(
        &self,
        _: &Executor<'_, Context>,
        _: &QueryTrail<'_, PageInfo, Walked>,
    ) -> FieldResult<&PageInfo> {
        Ok(&self.page_info)
    }

    fn field_total_count(&self, _: &Executor<'_, Context>) -> FieldResult<&i32> {
        Ok(&self.total_count)
    }
}

pub struct Edge<T> {
    node: T,
    cursor: Cursor,
}

pub type UserEdge = Edge<User>;

impl UserEdgeFields for UserEdge {
    fn field_node(
        &self,
        _: &Executor<'_, Context>,
        _: &QueryTrail<'_, User, Walked>,
    ) -> FieldResult<&User> {
        Ok(&self.node)
    }

    fn field_cursor(&self, _: &Executor<'_, Context>) -> FieldResult<&Cursor> {
        Ok(&self.cursor)
    }
}
