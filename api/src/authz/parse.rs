//! Parser for the rule language of design §4.1.
//!
//! Scannerless recursive descent over the schema text. There is no separate token stream because
//! two constructs need the raw source — the `where` clause of a derivation, and identifiers that
//! carry `:` and `.` (`service:app:system-settings`, `org:credit`, `sites.manager_id`) — and
//! threading a lexer mode through a token stream to serve two cases costs more than it saves.
//!
//! Grammar, as implemented. Four additions to the design's sketch, each because the compiler
//! cannot do its job without the fact and inferring it would mean guessing:
//!
//! ```text
//! file        := ( definition | extension | namespace )*
//! definition  := 'definition' NAME [ 'on' TABLE '.' COL [ 'int' | 'uuid' ] ] '{' member* '}'
//! extension   := 'extend' NAME '{' member* '}'
//! member      := 'closure'    NAME 'via' NAME
//!              | 'relation'   NAME ':' relbody
//!              | 'permission' NAME '=' expr
//!              | 'scope'      NAME
//! relbody     := TYPE '=' derivation                     // derived
//!              | subject ( '|' subject )*                // stored
//! subject     := TYPE | TYPE '#' RELATION | TYPE ':*'
//! derivation  := 'column' '(' TABLE '.' COL '->' TARGET
//!                            [ 'of'    TABLE '.' COL ]
//!                            [ 'via'   TABLE '.' COL ( ',' TABLE '.' COL )* ]
//!                            [ 'where' <raw sql to the closing paren> ] ')'
//! namespace   := 'namespace' NS [ 'service' ] [ '=' NAME ]
//!
//! expr        := excl
//! excl        := inter ( '-' inter )*        // loosest
//! inter       := union ( '&' union )*
//! union       := term  ( '|' term  )*        // tightest
//! term        := '(' expr ')' | NAME [ '^' ] | NAME '->' term | TYPE ':' ID '->' term
//! ```
//!
//! * **`on TABLE.COL [int|uuid]`** — where a definition's objects live, and what type its id is.
//!   Without it the compiler cannot write the object-id expression for a list filter, `^` cannot
//!   know whether the id is an integer the closure tables can be joined on, and a `uuid` id would
//!   be compared to `relation_tuples.resource_id` (text) with no cast, which Postgres refuses. Definitions with no table (`service`,
//!   `platform`, `delegation_scope`) omit it and are reachable only by an explicit object id.
//! * **`of TABLE.COL`** — the userset direction. `column(users.id -> user of users.organization_id)`
//!   is "the users whose organization is this object", which is what `organization#member` means;
//!   without the clause it is indistinguishable from `column(board_posts.user_id -> user)`, which
//!   means the opposite.
//! * **`extend NAME`** — further members for a definition declared in **another file**. A rule file
//!   may not redeclare a definition another file already declared, so without this a consumer could
//!   not add its own resource families to `organization`: the framework would have to enumerate
//!   them, which means the framework has to know what its consumers are. An extension may not
//!   introduce `on` or `closure` — those are the definition's identity, not a member of it — and a
//!   member that collides with one the target already has is refused.
//! * **`namespace`** — the legacy-namespace inventory validation rule 8 checks against.

use super::ir::*;

/// Marks every own-table derivation as uncorrelated.
///
/// A definition with no backing table has no row to correlate a derivation to. Kept as a free
/// function because an extension's members are parsed before the target definition is known, so the
/// registry applies it once the two are joined.
pub fn free_correlate(relations: &mut [Relation]) {
  for relation in relations {
    if let RelationSource::Derived(derivation) = &mut relation.source
      && derivation.correlation == Correlation::Own
    {
      derivation.correlation = Correlation::Free;
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
  pub file:    String,
  pub line:    usize,
  pub column:  usize,
  pub message: String,
}

impl std::fmt::Display for ParseError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}:{}:{}: {}", self.file, self.line, self.column, self.message)
  }
}

/// One parsed rule file.
#[derive(Debug, Default)]
pub struct SchemaFile {
  pub definitions: Vec<Definition>,
  pub extensions:  Vec<DefinitionExtension>,
  pub namespaces:  Vec<NamespaceEntry>,
}

pub fn parse(file: &str, src: &str) -> Result<SchemaFile, ParseError> {
  Parser {
    file: file.to_owned(),
    src:  src.as_bytes(),
    text: src,
    pos:  0,
  }
  .file()
}

struct Parser<'a> {
  file: String,
  src:  &'a [u8],
  text: &'a str,
  pos:  usize,
}

impl<'a> Parser<'a> {
  // ── cursor ────────────────────────────────────────────────────────────────────────────────

  fn err<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
    let consumed = &self.text[..self.pos.min(self.text.len())];
    let line = consumed.matches('\n').count() + 1;
    let column = consumed.rsplit('\n').next().map(str::len).unwrap_or(0) + 1;
    Err(ParseError {
      file: self.file.clone(),
      line,
      column,
      message: message.into(),
    })
  }

  fn skip(&mut self) {
    loop {
      while self.pos < self.src.len() && self.src[self.pos].is_ascii_whitespace() {
        self.pos += 1;
      }
      if self.src[self.pos..].starts_with(b"//") {
        while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
          self.pos += 1;
        }
        continue;
      }
      break;
    }
  }

  fn peek(&mut self) -> Option<u8> {
    self.skip();
    self.src.get(self.pos).copied()
  }

  fn eat(&mut self, c: u8) -> bool {
    if self.peek() == Some(c) {
      self.pos += 1;
      true
    } else {
      false
    }
  }

  fn expect(&mut self, c: u8) -> Result<(), ParseError> {
    if self.eat(c) { Ok(()) } else { self.err(format!("expected `{}`", c as char)) }
  }

  /// `->`, which must be tried before `-`.
  fn eat_arrow(&mut self) -> bool {
    self.skip();
    if self.src[self.pos..].starts_with(b"->") {
      self.pos += 2;
      true
    } else {
      false
    }
  }

  fn at_arrow(&mut self) -> bool {
    self.skip();
    self.src[self.pos..].starts_with(b"->")
  }

  /// An identifier, which may carry `:` and `.` internally.
  ///
  /// `-` continues an identifier only when a word character follows it, so `system-settings` is one
  /// name while `board->view` and `edit - blocked` are not. `:` and `.` continue likewise, which is
  /// what makes `service:app:system-settings`, `org:credit` and `sites.manager_id` single
  /// names — and keeps `user:*` and `relation viewer: user` three tokens, because `*` and a space
  /// are not word characters.
  fn ident(&mut self) -> Result<String, ParseError> {
    self.skip();
    let start = self.pos;
    if !matches!(self.src.get(self.pos), Some(c) if c.is_ascii_alphabetic() || *c == b'_') {
      return self.err("expected a name");
    }
    self.pos += 1;
    while let Some(&c) = self.src.get(self.pos) {
      let word = c.is_ascii_alphanumeric() || c == b'_';
      let joiner = matches!(c, b'-' | b':' | b'.')
        && matches!(self.src.get(self.pos + 1), Some(n) if n.is_ascii_alphanumeric() || *n == b'_');
      if word || joiner {
        self.pos += 1;
      } else {
        break;
      }
    }
    Ok(self.text[start..self.pos].to_owned())
  }

  /// Consumes `name` if it is the next identifier. Used for the keywords, which are contextual
  /// rather than reserved — `scope` is a member keyword and is also a perfectly good relation name.
  fn eat_keyword(&mut self, name: &str) -> bool {
    let save = self.pos;
    match self.ident() {
      Ok(id) if id == name => true,
      _ => {
        self.pos = save;
        false
      }
    }
  }

  fn split_qualified(&self, name: &str) -> Option<(String, String)> {
    let (table, column) = name.split_once('.')?;
    if table.is_empty() || column.is_empty() || column.contains('.') {
      return None;
    }
    Some((table.to_owned(), column.to_owned()))
  }

  // ── file ──────────────────────────────────────────────────────────────────────────────────

  fn file(mut self) -> Result<SchemaFile, ParseError> {
    let mut out = SchemaFile::default();
    while self.peek().is_some() {
      if self.eat_keyword("definition") {
        out.definitions.push(self.definition()?);
      } else if self.eat_keyword("extend") {
        out.extensions.push(self.extension()?);
      } else if self.eat_keyword("namespace") {
        out.namespaces.push(self.namespace()?);
      } else {
        return self.err("expected `definition`, `extend` or `namespace`");
      }
    }
    Ok(out)
  }

  fn namespace(&mut self) -> Result<NamespaceEntry, ParseError> {
    let namespace = self.ident()?;
    let gated = self.eat_keyword("service");
    let definition = if self.eat(b'=') { Some(self.ident()?) } else { None };
    if !gated && definition.is_none() {
      return self.err(format!(
        "namespace `{namespace}` declares neither a route gate (`service`) nor a definition (`= <def>`), \
         so nothing would ever evaluate it"
      ));
    }
    Ok(NamespaceEntry {
      namespace,
      gated,
      definition,
      source: self.file.clone(),
    })
  }

  // ── definition ────────────────────────────────────────────────────────────────────────────

  fn definition(&mut self) -> Result<Definition, ParseError> {
    let name = self.ident()?;
    let table = if self.eat_keyword("on") {
      let qualified = self.ident()?;
      let Some((table, id_col)) = self.split_qualified(&qualified) else {
        return self.err(format!("`on {qualified}` must be `<table>.<column>`"));
      };
      let kind = if self.eat_keyword("int") {
        IdKind::Int
      } else if self.eat_keyword("uuid") {
        IdKind::Uuid
      } else {
        IdKind::Text
      };
      Some(ResourceTable {
        table,
        id_col,
        kind,
      })
    } else {
      None
    };

    self.expect(b'{')?;
    let mut def = Definition {
      name,
      table,
      closure: None,
      relations: Vec::new(),
      permissions: Vec::new(),
    };

    self.members(&mut def, false)?;

    // A definition with no backing table has no row to correlate a derivation to, so its
    // derivations are uncorrelated by construction. Recording that here rather than re-deriving it
    // in the compiler keeps `Correlation` a fact of the registry instead of a compile-time guess.
    if def.table.is_none() {
      free_correlate(&mut def.relations);
    }

    Ok(def)
  }

  /// `extend NAME { member* }` — further members for a definition another file declared.
  fn extension(&mut self) -> Result<DefinitionExtension, ParseError> {
    let target = self.ident()?;

    // `on` and `closure` are the definition's identity, not members of it. An extension that could
    // move a definition's table, id kind or hierarchy would be redeclaring it under another name.
    if self.eat_keyword("on") {
      return self.err(format!(
        "`extend {target}` cannot declare `on`; where a definition's objects live is \
                      part of the definition, and only the file that declares it may say"
      ));
    }

    self.expect(b'{')?;
    let mut def = Definition {
      name:        target.clone(),
      table:       None,
      closure:     None,
      relations:   Vec::new(),
      permissions: Vec::new(),
    };

    self.members(&mut def, true)?;

    Ok(DefinitionExtension {
      target,
      relations: def.relations,
      permissions: def.permissions,
    })
  }

  /// The shared `{ member* }` body, up to and including the closing brace.
  ///
  /// `in_extension` refuses `closure`, which an extension may not introduce. Everything else is
  /// identical, deliberately: an extension is not a second language, it is the same members applied
  /// to a definition that lives in another file.
  fn members(&mut self, def: &mut Definition, in_extension: bool) -> Result<(), ParseError> {
    while !self.eat(b'}') {
      if self.peek().is_none() {
        let what = if in_extension { "extension of" } else { "definition" };
        return self.err(format!("{what} `{}` is not closed", def.name));
      }
      if self.eat_keyword("closure") {
        if in_extension {
          return self.err(format!(
            "`extend {}` cannot declare a closure; a definition's hierarchy is part of the \
             definition, and only the file that declares it may say",
            def.name
          ));
        }
        let relation = self.ident()?;
        if !self.eat_keyword("via") {
          return self.err("expected `via <closure table>`");
        }
        let table = self.ident()?;
        if def.closure.is_some() {
          return self.err(format!("definition `{}` declares more than one closure", def.name));
        }
        def.closure = Some(ClosureDecl { relation, table });
      } else if self.eat_keyword("relation") {
        let relation = self.relation()?;
        if def.relation(&relation.name).is_some() {
          return self.err(format!("relation `{}` is declared twice", relation.name));
        }
        def.relations.push(relation);
      } else if self.eat_keyword("permission") {
        let name = self.ident()?;
        self.expect(b'=')?;
        let rewrite = self.expr()?;
        if def.permission(&name).is_some() {
          return self.err(format!("permission `{name}` is declared twice"));
        }
        def.permissions.push((name, rewrite));
      } else if self.eat_keyword("scope") {
        let family = self.ident()?;
        if def.relation(&format!("{family}_viewer")).is_some() {
          return self.err(format!("scope `{family}` is declared twice"));
        }
        self.expand_scope(def, &family);
      } else {
        let extra = if in_extension { "" } else { "`closure`, " };
        return self.err(format!("expected `relation`, `permission`, {extra}`scope` or `}}`"));
      }
    }
    Ok(())
  }

  /// The `scope <family>` shorthand of §5.2.
  ///
  /// Text-level only: it expands here, before the IR, into three relations and three permissions
  /// that are ordinary canonical Zanzibar. It exists so the eleven-odd families on `organization`
  /// are not written out 3×N times, where a copy-paste slip would be a silent authority change.
  fn expand_scope(&self, def: &mut Definition, family: &str) {
    let subjects = vec![
      SubjectSpec::Direct("user".to_owned()),
      SubjectSpec::Userset {
        ty:       "role".to_owned(),
        relation: "member".to_owned(),
      },
      SubjectSpec::Userset {
        ty:       "organization".to_owned(),
        relation: "member".to_owned(),
      },
      SubjectSpec::Direct("apikey".to_owned()),
    ];

    for tier in ["viewer", "editor", "manager"] {
      def.relations.push(Relation {
        name:   format!("{family}_{tier}"),
        source: RelationSource::Stored {
          subjects:        subjects.clone(),
          allows_wildcard: false,
        },
      });
    }

    // Each tier is "a tuple at this tier on this organization or any ancestor, or the tier above".
    // `manage` is the definition's blanket manager permission, so one `organization:100#manager`
    // tuple is the whole of R2 for that person.
    def.permissions.push((
      format!("view_{family}"),
      Rewrite::Union(vec![
        Rewrite::Closure(format!("{family}_viewer")),
        Rewrite::Computed(format!("edit_{family}")),
      ]),
    ));
    def.permissions.push((
      format!("edit_{family}"),
      Rewrite::Union(vec![
        Rewrite::Closure(format!("{family}_editor")),
        Rewrite::Computed(format!("manage_{family}")),
      ]),
    ));
    def.permissions.push((
      format!("manage_{family}"),
      Rewrite::Union(vec![
        Rewrite::Closure(format!("{family}_manager")),
        Rewrite::Computed("manage".to_owned()),
      ]),
    ));
  }

  fn relation(&mut self) -> Result<Relation, ParseError> {
    let name = self.ident()?;
    self.expect(b':')?;

    let first = self.subject_spec()?;
    if self.eat(b'=') {
      let SubjectSpec::Direct(target) = first else {
        return self.err("a derived relation's declared target must be a plain type");
      };
      let derivation = self.derivation(&target)?;
      return Ok(Relation {
        name,
        source: RelationSource::Derived(derivation),
      });
    }

    let mut subjects = vec![first];
    while self.eat(b'|') {
      subjects.push(self.subject_spec()?);
    }
    let allows_wildcard = subjects.iter().any(|s| matches!(s, SubjectSpec::Wildcard(_)));

    Ok(Relation {
      name,
      source: RelationSource::Stored {
        subjects,
        allows_wildcard,
      },
    })
  }

  fn subject_spec(&mut self) -> Result<SubjectSpec, ParseError> {
    let ty = self.ident()?;
    if self.eat(b'#') {
      let relation = self.ident()?;
      return Ok(SubjectSpec::Userset { ty, relation });
    }
    if self.eat(b':') {
      if !self.eat(b'*') {
        return self.err("the only subject id a declaration may name is the wildcard `*`");
      }
      return Ok(SubjectSpec::Wildcard(ty));
    }
    Ok(SubjectSpec::Direct(ty))
  }

  fn derivation(&mut self, target_type: &str) -> Result<Derivation, ParseError> {
    if !self.eat_keyword("column") {
      return self.err("a derived relation's source must be `column(...)`");
    }
    self.expect(b'(')?;

    let qualified = self.ident()?;
    let Some((table, column)) = self.split_qualified(&qualified) else {
      return self.err(format!("`{qualified}` must be `<table>.<column>`"));
    };
    if !self.eat_arrow() {
      return self.err("expected `->` after the source column");
    }
    let named_target = self.ident()?;

    let target = match named_target.as_str() {
      "subject_key" => EdgeTarget::SubjectKey,
      "user" => EdgeTarget::User,
      other => EdgeTarget::Object(other.to_owned()),
    };
    // The declared type (`relation owner: user = …`) and the arrow's target must agree, or one of
    // the two is a lie and the compiler would silently follow the wrong one.
    let declared_matches = match &target {
      EdgeTarget::SubjectKey => target_type == "user",
      EdgeTarget::User => target_type == "user",
      EdgeTarget::Object(name) => target_type == name,
    };
    if !declared_matches {
      return self.err(format!("relation declares `{target_type}` but derives `{named_target}`"));
    }

    let correlation = if self.eat_keyword("of") {
      let qualified = self.ident()?;
      let Some((of_table, of_column)) = self.split_qualified(&qualified) else {
        return self.err(format!("`of {qualified}` must be `<table>.<column>`"));
      };
      if of_table != table {
        return self
          .err(format!("`of {of_table}.{of_column}` names a different table from `{table}.{column}`"));
      }
      Correlation::Of(of_column)
    } else {
      Correlation::Own
    };

    let mut hops = Vec::new();
    if self.eat_keyword("via") {
      loop {
        let qualified = self.ident()?;
        let Some((hop_table, next_col)) = self.split_qualified(&qualified) else {
          return self.err(format!("`via {qualified}` must be `<table>.<column>`"));
        };
        hops.push(Hop {
          table: hop_table,
          key_col: "id".to_owned(),
          next_col,
        });
        if !self.eat(b',') {
          break;
        }
      }
    }

    let filter = if self.eat_keyword("where") { Some(self.raw_until_close_paren()?) } else { None };

    self.expect(b')')?;

    Ok(Derivation {
      table,
      column,
      target,
      correlation,
      hops,
      filter,
    })
  }

  /// Raw SQL up to the `)` that closes the enclosing `column(`.
  fn raw_until_close_paren(&mut self) -> Result<String, ParseError> {
    self.skip();
    let start = self.pos;
    let mut depth = 0usize;
    while let Some(&c) = self.src.get(self.pos) {
      match c {
        b'(' => depth += 1,
        b')' if depth == 0 => break,
        b')' => depth -= 1,
        _ => {}
      }
      self.pos += 1;
    }
    if self.pos >= self.src.len() {
      return self.err("unterminated `where` clause");
    }
    Ok(self.text[start..self.pos].trim().to_owned())
  }

  // ── expressions ───────────────────────────────────────────────────────────────────────────

  fn expr(&mut self) -> Result<Rewrite, ParseError> {
    self.exclusion()
  }

  fn exclusion(&mut self) -> Result<Rewrite, ParseError> {
    let mut base = self.intersection()?;
    // `-` is left-associative, so `a - b - c` is `(a - b) - c`, which is `a - (b | c)`.
    while self.peek() == Some(b'-') && !self.at_arrow() {
      self.pos += 1;
      base = Rewrite::Exclusion {
        base:     Box::new(base),
        subtract: Box::new(self.intersection()?),
      };
    }
    Ok(base)
  }

  fn intersection(&mut self) -> Result<Rewrite, ParseError> {
    let mut parts = vec![self.union()?];
    while self.eat(b'&') {
      parts.push(self.union()?);
    }
    Ok(if parts.len() == 1 { parts.pop().unwrap() } else { Rewrite::Intersection(parts) })
  }

  fn union(&mut self) -> Result<Rewrite, ParseError> {
    let mut parts = vec![self.term()?];
    while self.eat(b'|') {
      parts.push(self.term()?);
    }
    Ok(if parts.len() == 1 { parts.pop().unwrap() } else { Rewrite::Union(parts) })
  }

  fn term(&mut self) -> Result<Rewrite, ParseError> {
    if self.eat(b'(') {
      let inner = self.expr()?;
      self.expect(b')')?;
      return Ok(inner);
    }

    let name = self.ident()?;

    if self.eat(b'^') {
      if self.at_arrow() {
        return self.err("a closure cannot be walked through; `^` must be the last step");
      }
      return Ok(Rewrite::Closure(name));
    }

    if self.eat_arrow() {
      let next = Box::new(self.term()?);
      // `service:<namespace>->…` is an absolute object; `board->…` is an edge on this object.
      return Ok(match name.split_once(':') {
        Some((ty, id)) => Rewrite::Absolute {
          ty: ty.to_owned(),
          id: id.to_owned(),
          next,
        },
        None => Rewrite::Walk { edge: name, next },
      });
    }

    if name.contains(':') {
      return self.err(format!("`{name}` names an object, which is only usable as `{name}-><permission>`"));
    }

    // Whether a bare name is a relation or a permission is resolved against the definition in
    // `validate`, which is the only place that knows both.
    Ok(Rewrite::This(name))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn one(src: &str) -> Definition {
    let mut file = parse("test.sz", src).expect("parses");
    file.definitions.pop().expect("a definition")
  }

  fn extension_of(src: &str) -> DefinitionExtension {
    let mut file = parse("test.sz", src).expect("parses");
    file.extensions.pop().expect("an extension")
  }

  #[test]
  fn stored_and_derived_relations_round_trip() {
    let def = one(
      r#"
      definition thing on things.id int {
        relation viewer: user | role#member | user:*
        relation owner:  user = column(things.user_id -> user where things.user_id IS NOT NULL)
        permission view = viewer | owner
      }
      "#,
    );

    assert_eq!(def.table.as_ref().unwrap().table, "things");
    assert_eq!(def.table.as_ref().unwrap().kind, IdKind::Int);
    let RelationSource::Stored {
      subjects,
      allows_wildcard,
    } = &def.relation("viewer").unwrap().source
    else {
      panic!("viewer must be stored")
    };
    assert_eq!(subjects.len(), 3);
    assert!(allows_wildcard, "`user:*` must set allows_wildcard");
    let RelationSource::Derived(d) = &def.relation("owner").unwrap().source else {
      panic!("owner must be derived")
    };
    assert_eq!((d.table.as_str(), d.column.as_str()), ("things", "user_id"));
    assert_eq!(d.filter.as_deref(), Some("things.user_id IS NOT NULL"));
  }

  /// `|` binds tightest, then `&`, then `-` (§4.1). If this ever flips, `board_post#edit` silently
  /// stops subtracting the block from the whole union and starts subtracting it from one branch.
  #[test]
  fn precedence_is_union_then_intersection_then_exclusion() {
    let def = one("definition t { permission p = a | b & c - d | e }");
    let Rewrite::Exclusion { base, subtract } = def.permission("p").unwrap() else {
      panic!("the top of the tree must be the exclusion")
    };
    assert!(matches!(**base, Rewrite::Intersection(_)));
    assert!(matches!(**subtract, Rewrite::Union(_)));
  }

  /// `a - b - c` is `(a - b) - c`, i.e. `a - (b | c)`.
  #[test]
  fn exclusion_is_left_associative() {
    let def = one("definition t { permission p = a - b - c }");
    let Rewrite::Exclusion { base, .. } = def.permission("p").unwrap() else { panic!() };
    assert!(matches!(**base, Rewrite::Exclusion { .. }));
  }

  /// The lexer must not swallow `->` into the identifier before it, and must keep a hyphenated
  /// service id whole.
  #[test]
  fn arrows_and_hyphenated_ids_lex_apart() {
    let def = one("definition t { permission p = board->view | service:demo:settings->can_read }");
    let Rewrite::Union(parts) = def.permission("p").unwrap() else { panic!() };
    assert!(matches!(&parts[0], Rewrite::Walk { edge, .. } if edge == "board"));
    let Rewrite::Absolute { ty, id, .. } = &parts[1] else { panic!() };
    assert_eq!((ty.as_str(), id.as_str()), ("service", "demo:settings"));
  }

  #[test]
  fn multi_hop_derivation_records_every_join() {
    let def = one(
      "definition t on buds.id int {
         relation root: demo_root = column(buds.leaf_id -> demo_root
                                           via leaves.branch_id, branches.root_id)
       }",
    );
    let RelationSource::Derived(d) = &def.relation("root").unwrap().source else {
      panic!()
    };
    assert_eq!(d.hops.len(), 2);
    assert_eq!((d.hops[0].table.as_str(), d.hops[0].next_col.as_str()), ("leaves", "branch_id"));
    assert_eq!((d.hops[1].table.as_str(), d.hops[1].next_col.as_str()), ("branches", "root_id"));
  }

  #[test]
  fn scope_shorthand_expands_to_three_tiers() {
    let def = one("definition organization on organizations.id int { scope task }");
    for tier in ["task_viewer", "task_editor", "task_manager"] {
      assert!(def.relation(tier).is_some(), "{tier} must exist");
    }
    for perm in ["view_task", "edit_task", "manage_task"] {
      assert!(def.permission(perm).is_some(), "{perm} must exist");
    }
  }

  #[test]
  fn a_closure_cannot_be_walked_through() {
    let err = parse("t.sz", "definition t { permission p = a^->b }").unwrap_err();
    assert!(err.message.contains("closure"), "{err}");
  }

  #[test]
  fn a_namespace_must_do_something() {
    let err = parse("t.sz", "namespace demo:widget").unwrap_err();
    assert!(err.message.contains("neither"), "{err}");
  }

  /// `extend` carries exactly the members a definition body does, `scope` included — which is the
  /// only member any file in the estate uses it for.
  #[test]
  fn an_extension_carries_the_same_members_a_definition_does() {
    let extension = extension_of(
      "extend organization {
         scope widget
         relation auditor: user
         permission audit = auditor
       }",
    );

    assert_eq!(extension.target, "organization");
    for tier in ["widget_viewer", "widget_editor", "widget_manager", "auditor"] {
      assert!(extension.relations.iter().any(|r| r.name == tier), "{tier} must exist");
    }
    for permission in ["view_widget", "edit_widget", "manage_widget", "audit"] {
      assert!(
        extension.permissions.iter().any(|(name, _)| name == permission),
        "{permission} must exist"
      );
    }
  }

  /// An extension may not say where the target's objects live, or what its hierarchy is. Both are
  /// the definition's identity: an extension that could move them would be redeclaring it.
  #[test]
  fn an_extension_cannot_restate_the_definitions_identity() {
    let on = parse("t.sz", "extend organization on orgs.id int { scope widget }").unwrap_err();
    assert!(on.message.contains("cannot declare `on`"), "{on}");

    let closure = parse("t.sz", "extend organization { closure parent via org_closure }").unwrap_err();
    assert!(closure.message.contains("cannot declare a closure"), "{closure}");
  }

  /// A file may hold definitions, extensions and namespaces in any order.
  #[test]
  fn a_file_may_mix_definitions_extensions_and_namespaces() {
    let file = parse(
      "t.sz",
      "extend organization { scope widget }
       definition demo_widget on widgets.slug { relation viewer: user  permission view = viewer }
       namespace demo:widget service = demo_widget",
    )
    .expect("parses");

    assert_eq!(file.definitions.len(), 1);
    assert_eq!(file.extensions.len(), 1);
    assert_eq!(file.namespaces.len(), 1);
    assert_eq!(file.namespaces[0].source, "t.sz");
  }
}
