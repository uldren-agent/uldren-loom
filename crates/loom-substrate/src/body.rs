use loom_codec::Value;
use loom_types::{LoomError, Result};

use crate::order_token::{OrderToken, first_token};
use crate::{codec_error, validate_text};

pub const BODY_SCHEMA: &str = "loom.substrate.body.v1";
pub const BLOCK_REF_RENDER_DEPTH_LIMIT: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
    Link(String),
}

impl Mark {
    fn to_value(&self) -> Value {
        match self {
            Mark::Bold => Value::Array(vec![Value::Uint(0)]),
            Mark::Italic => Value::Array(vec![Value::Uint(1)]),
            Mark::Underline => Value::Array(vec![Value::Uint(2)]),
            Mark::Strike => Value::Array(vec![Value::Uint(3)]),
            Mark::Code => Value::Array(vec![Value::Uint(4)]),
            Mark::Link(href) => Value::Array(vec![Value::Uint(5), Value::Text(href.clone())]),
        }
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = crate::Fields::array(value, "mark")?;
        let tag = fields.uint("mark tag")?;
        let mark = match tag {
            0 => Mark::Bold,
            1 => Mark::Italic,
            2 => Mark::Underline,
            3 => Mark::Strike,
            4 => Mark::Code,
            5 => Mark::Link(fields.text("href")?),
            other => {
                return Err(LoomError::corrupt(format!("unknown mark tag {other}")));
            }
        };
        fields.end("mark")?;
        Ok(mark)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRun {
    pub text: String,
    pub marks: Vec<Mark>,
}

impl TextRun {
    pub fn new(text: impl Into<String>, marks: Vec<Mark>) -> Result<Self> {
        let text = text.into();
        if text.is_empty() {
            return Err(LoomError::invalid("text run must not be empty"));
        }
        let mut marks = marks;
        marks.sort();
        marks.dedup();
        for mark in &marks {
            if let Mark::Link(href) = mark {
                validate_text("link href", href)?;
            }
        }
        Ok(Self { text, marks })
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.text.clone()),
            Value::Array(self.marks.iter().map(Mark::to_value).collect()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = crate::Fields::array(value, "text run")?;
        let text = fields.text("text")?;
        let marks = match fields.next("marks")? {
            Value::Array(items) => items
                .into_iter()
                .map(Mark::from_value)
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(LoomError::corrupt("marks must be an array")),
        };
        fields.end("text run")?;
        TextRun::new(text, marks)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading {
        level: u8,
    },
    ListItem {
        ordered: bool,
    },
    CodeBlock {
        language: String,
    },
    Quote,
    Divider,
    Embed,
    BlockRef {
        entity_id: String,
        block_id: Option<String>,
        section: bool,
        pin: Option<u64>,
    },
    Opaque {
        kind: String,
        payload: Vec<u8>,
    },
}

impl BlockKind {
    fn to_value(&self) -> Value {
        match self {
            BlockKind::Paragraph => Value::Array(vec![Value::Uint(0)]),
            BlockKind::Heading { level } => {
                Value::Array(vec![Value::Uint(1), Value::Uint(u64::from(*level))])
            }
            BlockKind::ListItem { ordered } => {
                Value::Array(vec![Value::Uint(2), Value::Bool(*ordered)])
            }
            BlockKind::CodeBlock { language } => {
                Value::Array(vec![Value::Uint(3), Value::Text(language.clone())])
            }
            BlockKind::Quote => Value::Array(vec![Value::Uint(4)]),
            BlockKind::Divider => Value::Array(vec![Value::Uint(5)]),
            BlockKind::Embed => Value::Array(vec![Value::Uint(6)]),
            BlockKind::BlockRef {
                entity_id,
                block_id,
                section,
                pin,
            } => Value::Array(vec![
                Value::Uint(7),
                Value::Text(entity_id.clone()),
                option_text(block_id.as_deref()),
                Value::Bool(*section),
                option_uint(*pin),
            ]),
            BlockKind::Opaque { kind, payload } => Value::Array(vec![
                Value::Uint(8),
                Value::Text(kind.clone()),
                Value::Bytes(payload.clone()),
            ]),
        }
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = crate::Fields::array(value, "block kind")?;
        let tag = fields.uint("block kind tag")?;
        let kind = match tag {
            0 => BlockKind::Paragraph,
            1 => {
                let level = u8::try_from(fields.uint("heading level")?)
                    .map_err(|_| LoomError::corrupt("heading level is too large"))?;
                BlockKind::Heading { level }
            }
            2 => BlockKind::ListItem {
                ordered: fields.bool("ordered")?,
            },
            3 => BlockKind::CodeBlock {
                language: fields.text("language")?,
            },
            4 => BlockKind::Quote,
            5 => BlockKind::Divider,
            6 => BlockKind::Embed,
            7 => BlockKind::BlockRef {
                entity_id: fields.text("entity_id")?,
                block_id: optional_text_value(fields.next("block_id")?, "block_id")?,
                section: fields.bool("section")?,
                pin: optional_uint_value(fields.next("pin")?, "pin")?,
            },
            8 => BlockKind::Opaque {
                kind: fields.text("kind")?,
                payload: fields.bytes("payload")?,
            },
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown block kind tag {other}"
                )));
            }
        };
        fields.end("block kind")?;
        validate_block_kind(&kind)?;
        Ok(kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub block_id: String,
    pub order: OrderToken,
    pub kind: BlockKind,
    pub runs: Vec<TextRun>,
    pub children: Vec<Block>,
}

impl Block {
    pub fn new(
        block_id: impl Into<String>,
        order: OrderToken,
        kind: BlockKind,
        runs: Vec<TextRun>,
        children: Vec<Block>,
    ) -> Result<Self> {
        let block_id = block_id.into();
        validate_text("block_id", &block_id)?;
        validate_block_kind(&kind)?;
        Ok(Self {
            block_id,
            order,
            kind,
            runs,
            children,
        }
        .normalized())
    }

    fn normalized(mut self) -> Self {
        self.runs = normalize_runs(self.runs);
        self.children = normalize_blocks(self.children);
        self
    }

    pub fn text_len(&self) -> usize {
        self.runs.iter().map(|run| run.text.len()).sum()
    }

    pub fn is_char_boundary(&self, offset: usize) -> bool {
        if offset > self.text_len() {
            return false;
        }
        let mut base = 0;
        for run in &self.runs {
            let end = base + run.text.len();
            if offset <= end {
                return run.text.is_char_boundary(offset - base);
            }
            base = end;
        }
        offset == 0
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.block_id.clone()),
            Value::Text(self.order.as_str().to_string()),
            self.kind.to_value(),
            Value::Array(self.runs.iter().map(TextRun::to_value).collect()),
            Value::Array(self.children.iter().map(Block::to_value).collect()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = crate::Fields::array(value, "block")?;
        let block_id = fields.text("block_id")?;
        let order = OrderToken::new(fields.text("order")?)?;
        let kind = BlockKind::from_value(fields.next("kind")?)?;
        let runs = match fields.next("runs")? {
            Value::Array(items) => items
                .into_iter()
                .map(TextRun::from_value)
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(LoomError::corrupt("block runs must be an array")),
        };
        let children = match fields.next("children")? {
            Value::Array(items) => items
                .into_iter()
                .map(Block::from_value)
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(LoomError::corrupt("block children must be an array")),
        };
        fields.end("block")?;
        Block::new(block_id, order, kind, runs, children)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Body {
    pub blocks: Vec<Block>,
}

impl Body {
    pub fn new(blocks: Vec<Block>) -> Self {
        Self {
            blocks: normalize_blocks(blocks),
        }
    }

    pub fn from_plain_text(text: impl Into<String>) -> Result<Self> {
        let text = text.into();
        if text.is_empty() {
            return Ok(Self::new(Vec::new()));
        }
        Ok(Self::new(vec![Block::new(
            "body",
            first_token(),
            BlockKind::Paragraph,
            vec![TextRun::new(text, Vec::new())?],
            Vec::new(),
        )?]))
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(BODY_SCHEMA.to_string()),
            Value::Array(self.blocks.iter().map(Block::to_value).collect()),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = crate::Fields::array(value, "body")?;
        outer.expect_text(BODY_SCHEMA)?;
        let blocks = match outer.next("blocks")? {
            Value::Array(items) => items
                .into_iter()
                .map(Block::from_value)
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(LoomError::corrupt("body blocks must be an array")),
        };
        outer.end("body")?;
        Ok(Body::new(blocks))
    }

    pub fn validate_range(&self, block_id: &str, start: usize, end: usize) -> Result<()> {
        let block = self
            .find_block(block_id)
            .ok_or_else(|| LoomError::not_found(format!("block {block_id}")))?;
        if start > end {
            return Err(LoomError::invalid("body range is inverted"));
        }
        if !block.is_char_boundary(start) || !block.is_char_boundary(end) {
            return Err(LoomError::invalid(
                "body range must use UTF-8 byte boundaries",
            ));
        }
        Ok(())
    }

    pub fn find_block(&self, block_id: &str) -> Option<&Block> {
        find_block(&self.blocks, block_id)
    }

    pub fn apply_patch(&self, patch: &BodyPatch, current_revision: u64) -> Result<Self> {
        if patch.base_revision != current_revision {
            return Err(LoomError::new(
                loom_types::Code::Conflict,
                "body patch base revision is stale",
            ));
        }
        let mut blocks = self.blocks.clone();
        for op in &patch.ops {
            apply_delta(&mut blocks, op)?;
        }
        Ok(Body::new(blocks))
    }

    pub fn render_text_with_refs<F>(&self, mut resolver: F) -> Result<BodyRender>
    where
        F: FnMut(&BlockRefTarget) -> Result<BlockRefResolution>,
    {
        let mut state = RenderState {
            resolver: &mut resolver,
            options: BodyRenderOptions::default(),
            stack: Vec::new(),
            tickets: Vec::new(),
            text: String::new(),
        };
        state.render_blocks(&self.blocks)?;
        Ok(BodyRender {
            text: state.text,
            tickets: state.tickets,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRender {
    pub text: String,
    pub tickets: Vec<BodyRenderIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRenderIssue {
    pub kind: BodyRenderIssueKind,
    pub entity_id: String,
    pub block_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyRenderIssueKind {
    MissingTarget,
    MissingBlock,
    Shredded,
    Cycle,
    DepthLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRenderOptions {
    pub max_block_ref_depth: usize,
}

impl Default for BodyRenderOptions {
    fn default() -> Self {
        Self {
            max_block_ref_depth: BLOCK_REF_RENDER_DEPTH_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRefTarget {
    pub entity_id: String,
    pub block_id: Option<String>,
    pub section: bool,
    pub pin: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockRefResolution {
    Found(Body),
    Missing,
    Shredded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyPatch {
    pub base_revision: u64,
    pub ops: Vec<BodyDelta>,
}

impl BodyPatch {
    pub fn new(base_revision: u64, ops: Vec<BodyDelta>) -> Result<Self> {
        if ops.is_empty() {
            return Err(LoomError::invalid(
                "body patch must contain at least one operation",
            ));
        }
        Ok(Self { base_revision, ops })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyAnchor {
    pub block_id: String,
    pub start: usize,
    pub end: usize,
    pub stale: bool,
}

impl BodyAnchor {
    pub fn new(block_id: impl Into<String>, start: usize, end: usize) -> Result<Self> {
        let block_id = block_id.into();
        validate_text("anchor block_id", &block_id)?;
        if start > end {
            return Err(LoomError::invalid("anchor range is inverted"));
        }
        Ok(Self {
            block_id,
            start,
            end,
            stale: false,
        })
    }

    pub fn map_splice(
        &self,
        block_id: &str,
        start: usize,
        end: usize,
        inserted_len: usize,
    ) -> Self {
        if self.stale || self.block_id != block_id {
            return self.clone();
        }
        if end <= self.start {
            let removed = end.saturating_sub(start);
            let delta = inserted_len as isize - removed as isize;
            return Self {
                start: shift_offset(self.start, delta),
                end: shift_offset(self.end, delta),
                ..self.clone()
            };
        }
        if start >= self.end {
            return self.clone();
        }
        Self {
            stale: true,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyEpoch {
    pub epoch_id: String,
    pub snapshot_revision: u64,
    pub delta_count: u64,
    pub key_retired: bool,
}

impl BodyEpoch {
    pub fn new(
        epoch_id: impl Into<String>,
        snapshot_revision: u64,
        delta_count: u64,
        key_retired: bool,
    ) -> Result<Self> {
        let epoch_id = epoch_id.into();
        validate_text("epoch_id", &epoch_id)?;
        Ok(Self {
            epoch_id,
            snapshot_revision,
            delta_count,
            key_retired,
        })
    }

    pub fn can_render(&self) -> bool {
        !self.key_retired
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyDelta {
    InsertBlock {
        parent_id: Option<String>,
        block: Block,
    },
    RemoveBlock {
        block_id: String,
    },
    MoveBlock {
        block_id: String,
        new_parent_id: Option<String>,
        new_order: OrderToken,
    },
    SetBlockKind {
        block_id: String,
        kind: BlockKind,
    },
    SpliceText {
        block_id: String,
        start: usize,
        end: usize,
        replacement: Vec<TextRun>,
    },
    ReplaceBody {
        body: Body,
    },
}

fn validate_block_kind(kind: &BlockKind) -> Result<()> {
    match kind {
        BlockKind::Heading { level } if !(1..=6).contains(level) => {
            Err(LoomError::invalid("heading level must be 1 through 6"))
        }
        BlockKind::CodeBlock { language } => validate_text("language", language),
        BlockKind::BlockRef {
            entity_id,
            block_id,
            pin: _,
            section: _,
        } => {
            validate_text("block_ref entity_id", entity_id)?;
            if let Some(block_id) = block_id {
                validate_text("block_ref block_id", block_id)?;
            }
            Ok(())
        }
        BlockKind::Opaque { kind, payload } => {
            validate_text("opaque block kind", kind)?;
            if payload.is_empty() {
                return Err(LoomError::invalid("opaque block payload must not be empty"));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn normalize_runs(runs: Vec<TextRun>) -> Vec<TextRun> {
    let mut out: Vec<TextRun> = Vec::new();
    for run in runs {
        if let Some(last) = out.last_mut()
            && last.marks == run.marks
        {
            last.text.push_str(&run.text);
            continue;
        }
        out.push(run);
    }
    out
}

fn normalize_blocks(mut blocks: Vec<Block>) -> Vec<Block> {
    blocks.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.block_id.cmp(&right.block_id))
    });
    blocks.into_iter().map(Block::normalized).collect()
}

fn apply_delta(blocks: &mut Vec<Block>, op: &BodyDelta) -> Result<()> {
    match op {
        BodyDelta::InsertBlock { parent_id, block } => {
            insert_block(blocks, parent_id.as_deref(), block.clone())
        }
        BodyDelta::RemoveBlock { block_id } => remove_block(blocks, block_id).map(|_| ()),
        BodyDelta::MoveBlock {
            block_id,
            new_parent_id,
            new_order,
        } => {
            let mut block = remove_block(blocks, block_id)?;
            block.order = new_order.clone();
            insert_block(blocks, new_parent_id.as_deref(), block)
        }
        BodyDelta::SetBlockKind { block_id, kind } => {
            validate_block_kind(kind)?;
            let block = find_block_mut(blocks, block_id)
                .ok_or_else(|| LoomError::not_found(format!("block {block_id}")))?;
            block.kind = kind.clone();
            Ok(())
        }
        BodyDelta::SpliceText {
            block_id,
            start,
            end,
            replacement,
        } => {
            let block = find_block_mut(blocks, block_id)
                .ok_or_else(|| LoomError::not_found(format!("block {block_id}")))?;
            splice_block_text(block, *start, *end, replacement.clone())
        }
        BodyDelta::ReplaceBody { body } => {
            *blocks = body.blocks.clone();
            Ok(())
        }
    }
}

fn insert_block(blocks: &mut Vec<Block>, parent_id: Option<&str>, block: Block) -> Result<()> {
    if find_block(blocks, &block.block_id).is_some() {
        return Err(LoomError::new(
            loom_types::Code::AlreadyExists,
            format!("block {}", block.block_id),
        ));
    }
    match parent_id {
        Some(parent_id) => {
            let parent = find_block_mut(blocks, parent_id)
                .ok_or_else(|| LoomError::not_found(format!("block {parent_id}")))?;
            parent.children.push(block);
            parent.children = normalize_blocks(std::mem::take(&mut parent.children));
        }
        None => {
            blocks.push(block);
            *blocks = normalize_blocks(std::mem::take(blocks));
        }
    }
    Ok(())
}

fn remove_block(blocks: &mut Vec<Block>, block_id: &str) -> Result<Block> {
    if let Some(index) = blocks.iter().position(|block| block.block_id == block_id) {
        return Ok(blocks.remove(index));
    }
    for block in blocks {
        if let Ok(removed) = remove_block(&mut block.children, block_id) {
            return Ok(removed);
        }
    }
    Err(LoomError::not_found(format!("block {block_id}")))
}

fn find_block<'a>(blocks: &'a [Block], block_id: &str) -> Option<&'a Block> {
    for block in blocks {
        if block.block_id == block_id {
            return Some(block);
        }
        if let Some(found) = find_block(&block.children, block_id) {
            return Some(found);
        }
    }
    None
}

fn find_block_mut<'a>(blocks: &'a mut [Block], block_id: &str) -> Option<&'a mut Block> {
    for block in blocks {
        if block.block_id == block_id {
            return Some(block);
        }
        if let Some(found) = find_block_mut(&mut block.children, block_id) {
            return Some(found);
        }
    }
    None
}

fn splice_block_text(
    block: &mut Block,
    start: usize,
    end: usize,
    replacement: Vec<TextRun>,
) -> Result<()> {
    if start > end {
        return Err(LoomError::invalid("splice range is inverted"));
    }
    if !block.is_char_boundary(start) || !block.is_char_boundary(end) {
        return Err(LoomError::invalid(
            "splice range must use UTF-8 byte boundaries",
        ));
    }
    let mut text = String::new();
    for run in &block.runs {
        text.push_str(&run.text);
    }
    text.replace_range(
        start..end,
        &replacement
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
    );
    block.runs = normalize_runs(replacement_runs(text, replacement));
    Ok(())
}

fn replacement_runs(text: String, replacement: Vec<TextRun>) -> Vec<TextRun> {
    if replacement.is_empty() {
        return TextRun::new(text, vec![])
            .map(|run| vec![run])
            .unwrap_or_default();
    }
    let marks = replacement
        .first()
        .map(|run| run.marks.clone())
        .unwrap_or_default();
    TextRun::new(text, marks)
        .map(|run| vec![run])
        .unwrap_or_default()
}

struct RenderState<'a, F>
where
    F: FnMut(&BlockRefTarget) -> Result<BlockRefResolution>,
{
    resolver: &'a mut F,
    options: BodyRenderOptions,
    stack: Vec<String>,
    tickets: Vec<BodyRenderIssue>,
    text: String,
}

impl<F> RenderState<'_, F>
where
    F: FnMut(&BlockRefTarget) -> Result<BlockRefResolution>,
{
    fn render_blocks(&mut self, blocks: &[Block]) -> Result<()> {
        for block in blocks {
            self.render_block(block)?;
        }
        Ok(())
    }

    fn render_block(&mut self, block: &Block) -> Result<()> {
        match &block.kind {
            BlockKind::BlockRef {
                entity_id,
                block_id,
                section,
                pin,
            } => self.render_block_ref(BlockRefTarget {
                entity_id: entity_id.clone(),
                block_id: block_id.clone(),
                section: *section,
                pin: *pin,
            })?,
            _ => {
                self.push_runs(&block.runs);
                self.render_blocks(&block.children)?;
            }
        }
        Ok(())
    }

    fn render_block_ref(&mut self, target: BlockRefTarget) -> Result<()> {
        let key = block_ref_key(&target);
        if self.stack.iter().any(|existing| existing == &key) {
            self.ticket(BodyRenderIssueKind::Cycle, &target);
            return Ok(());
        }
        if self.stack.len() >= self.options.max_block_ref_depth {
            self.ticket(BodyRenderIssueKind::DepthLimit, &target);
            return Ok(());
        }
        match (self.resolver)(&target)? {
            BlockRefResolution::Found(body) => {
                self.stack.push(key);
                if let Some(block_id) = &target.block_id {
                    match body.find_block(block_id) {
                        Some(block) => {
                            self.render_block(block)?;
                        }
                        None => self.ticket(BodyRenderIssueKind::MissingBlock, &target),
                    }
                } else {
                    self.render_blocks(&body.blocks)?;
                }
                self.stack.pop();
            }
            BlockRefResolution::Missing => self.ticket(BodyRenderIssueKind::MissingTarget, &target),
            BlockRefResolution::Shredded => self.ticket(BodyRenderIssueKind::Shredded, &target),
        }
        Ok(())
    }

    fn push_runs(&mut self, runs: &[TextRun]) {
        if runs.is_empty() {
            return;
        }
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
        for run in runs {
            self.text.push_str(&run.text);
        }
        self.text.push('\n');
    }

    fn ticket(&mut self, kind: BodyRenderIssueKind, target: &BlockRefTarget) {
        self.tickets.push(BodyRenderIssue {
            kind: kind.clone(),
            entity_id: target.entity_id.clone(),
            block_id: target.block_id.clone(),
        });
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
        self.text.push_str(&format!(
            "[block_ref:{}:{}]\n",
            target.entity_id,
            issue_token(&kind)
        ));
    }
}

fn block_ref_key(target: &BlockRefTarget) -> String {
    format!(
        "{}#{}@{}",
        target.entity_id,
        target.block_id.as_deref().unwrap_or("*"),
        target
            .pin
            .map(|pin| pin.to_string())
            .unwrap_or_else(|| "latest".to_string())
    )
}

fn issue_token(kind: &BodyRenderIssueKind) -> &'static str {
    match kind {
        BodyRenderIssueKind::MissingTarget => "missing",
        BodyRenderIssueKind::MissingBlock => "missing_block",
        BodyRenderIssueKind::Shredded => "shredded",
        BodyRenderIssueKind::Cycle => "cycle",
        BodyRenderIssueKind::DepthLimit => "depth",
    }
}

fn shift_offset(offset: usize, delta: isize) -> usize {
    if delta.is_negative() {
        offset.saturating_sub(delta.unsigned_abs())
    } else {
        offset.saturating_add(delta as usize)
    }
}

fn option_text(value: Option<&str>) -> Value {
    value
        .map(|value| Value::Text(value.to_string()))
        .unwrap_or(Value::Null)
}

fn option_uint(value: Option<u64>) -> Value {
    value.map(Value::Uint).unwrap_or(Value::Null)
}

fn optional_text_value(value: Value, name: &str) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::Text(value) => Ok(Some(value)),
        _ => Err(LoomError::corrupt(format!("{name} must be text or null"))),
    }
}

fn optional_uint_value(value: Value, name: &str) -> Result<Option<u64>> {
    match value {
        Value::Null => Ok(None),
        Value::Uint(value) => Ok(Some(value)),
        _ => Err(LoomError::corrupt(format!("{name} must be uint or null"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(hex: &str) -> OrderToken {
        OrderToken::new(hex).unwrap()
    }

    fn paragraph(block_id: &str, text: &str) -> Block {
        Block::new(
            block_id,
            token("40000000000000000000000000000000"),
            BlockKind::Paragraph,
            vec![TextRun::new(text, vec![]).unwrap()],
            vec![],
        )
        .unwrap()
    }

    fn block_ref(block_id: &str, entity_id: &str, target_block: Option<&str>) -> Block {
        Block::new(
            block_id,
            token("40000000000000000000000000000000"),
            BlockKind::BlockRef {
                entity_id: entity_id.to_string(),
                block_id: target_block.map(str::to_string),
                section: false,
                pin: None,
            },
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    #[test]
    fn body_normalizes_runs_and_block_order() {
        let body = Body::new(vec![
            Block::new(
                "b",
                token("80000000000000000000000000000000"),
                BlockKind::Paragraph,
                vec![TextRun::new("two", vec![]).unwrap()],
                vec![],
            )
            .unwrap(),
            Block::new(
                "a",
                token("40000000000000000000000000000000"),
                BlockKind::Paragraph,
                vec![
                    TextRun::new("he", vec![Mark::Bold]).unwrap(),
                    TextRun::new("llo", vec![Mark::Bold]).unwrap(),
                ],
                vec![],
            )
            .unwrap(),
        ]);

        assert_eq!(body.blocks[0].block_id, "a");
        assert_eq!(body.blocks[0].runs.len(), 1);
        assert_eq!(body.blocks[0].runs[0].text, "hello");
        assert!(!body.encode().unwrap().is_empty());
    }

    #[test]
    fn body_validates_utf8_byte_ranges() {
        let body = Body::new(vec![
            Block::new(
                "a",
                token("40000000000000000000000000000000"),
                BlockKind::Paragraph,
                vec![TextRun::new("éx", vec![]).unwrap()],
                vec![],
            )
            .unwrap(),
        ]);

        assert!(body.validate_range("a", 0, 2).is_ok());
        assert!(body.validate_range("a", 1, 2).is_err());
        assert!(body.validate_range("a", 0, 3).is_ok());
    }

    #[test]
    fn block_ref_and_opaque_blocks_encode() {
        let body = Body::new(vec![
            Block::new(
                "ref",
                token("40000000000000000000000000000000"),
                BlockKind::BlockRef {
                    entity_id: "PAGE-1".to_string(),
                    block_id: Some("intro".to_string()),
                    section: true,
                    pin: Some(7),
                },
                vec![],
                vec![],
            )
            .unwrap(),
            Block::new(
                "opaque",
                token("80000000000000000000000000000000"),
                BlockKind::Opaque {
                    kind: "macro_reference".to_string(),
                    payload: vec![1, 2, 3],
                },
                vec![],
                vec![],
            )
            .unwrap(),
        ]);
        assert!(!body.encode().unwrap().is_empty());
    }

    #[test]
    fn plain_text_body_round_trips_through_canonical_body() {
        let body = Body::from_plain_text("hello\nworld").unwrap();
        let encoded = body.encode().unwrap();
        let decoded = Body::decode(&encoded).unwrap();
        let rendered = decoded
            .render_text_with_refs(|_| Ok(BlockRefResolution::Missing))
            .unwrap();
        assert_eq!(rendered.text, "hello\nworld\n");
    }

    #[test]
    fn body_round_trips_canonical_encoding() {
        let body = Body::new(vec![
            Block::new(
                "ref",
                token("40000000000000000000000000000000"),
                BlockKind::BlockRef {
                    entity_id: "page:target".to_string(),
                    block_id: Some("intro".to_string()),
                    section: true,
                    pin: Some(7),
                },
                vec![],
                vec![paragraph("child", "nested")],
            )
            .unwrap(),
            Block::new(
                "opaque",
                token("80000000000000000000000000000000"),
                BlockKind::Opaque {
                    kind: "macro_reference".to_string(),
                    payload: vec![1, 2, 3],
                },
                vec![],
                vec![],
            )
            .unwrap(),
        ]);

        let decoded = Body::decode(&body.encode().unwrap()).unwrap();

        assert_eq!(decoded, body);
    }

    #[test]
    fn block_ref_renderer_reads_through_without_caching() {
        let source = Body::new(vec![block_ref("ref", "page:target", Some("intro"))]);
        let target = Body::new(vec![paragraph("intro", "hello")]);

        let rendered = source
            .render_text_with_refs(|request| {
                assert_eq!(request.entity_id, "page:target");
                assert_eq!(request.block_id.as_deref(), Some("intro"));
                Ok(BlockRefResolution::Found(target.clone()))
            })
            .unwrap();

        assert_eq!(rendered.text, "hello\n");
        assert!(rendered.tickets.is_empty());
    }

    #[test]
    fn block_ref_renderer_reports_missing_and_shredded_targets() {
        let source = Body::new(vec![
            block_ref("missing", "page:missing", None),
            block_ref("shredded", "page:shredded", None),
        ]);

        let rendered = source
            .render_text_with_refs(|request| {
                if request.entity_id == "page:shredded" {
                    Ok(BlockRefResolution::Shredded)
                } else {
                    Ok(BlockRefResolution::Missing)
                }
            })
            .unwrap();

        assert_eq!(rendered.tickets.len(), 2);
        assert_eq!(rendered.tickets[0].kind, BodyRenderIssueKind::MissingTarget);
        assert_eq!(rendered.tickets[1].kind, BodyRenderIssueKind::Shredded);
        assert!(rendered.text.contains("[block_ref:page:missing:missing]"));
        assert!(rendered.text.contains("[block_ref:page:shredded:shredded]"));
    }

    #[test]
    fn block_ref_renderer_reports_cycles_and_depth_limit() {
        let source = Body::new(vec![block_ref("root", "page:loop", None)]);
        let loop_body = Body::new(vec![block_ref("loop", "page:loop", None)]);

        let cycle = source
            .render_text_with_refs(|_| Ok(BlockRefResolution::Found(loop_body.clone())))
            .unwrap();

        assert_eq!(cycle.tickets.len(), 1);
        assert_eq!(cycle.tickets[0].kind, BodyRenderIssueKind::Cycle);

        let mut count = 0;
        let deep = source
            .render_text_with_refs(|_| {
                count += 1;
                Ok(BlockRefResolution::Found(Body::new(vec![block_ref(
                    &format!("deep-{count}"),
                    &format!("page:{count}"),
                    None,
                )])))
            })
            .unwrap();

        assert_eq!(deep.tickets.len(), 1);
        assert_eq!(deep.tickets[0].kind, BodyRenderIssueKind::DepthLimit);
        assert_eq!(count, BLOCK_REF_RENDER_DEPTH_LIMIT);
    }

    #[test]
    fn body_patch_rejects_stale_base() {
        let body = Body::new(vec![
            Block::new(
                "a",
                token("40000000000000000000000000000000"),
                BlockKind::Paragraph,
                vec![TextRun::new("text", vec![]).unwrap()],
                vec![],
            )
            .unwrap(),
        ]);
        let patch = BodyPatch::new(
            1,
            vec![BodyDelta::RemoveBlock {
                block_id: "a".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(
            body.apply_patch(&patch, 2).unwrap_err().code,
            loom_types::Code::Conflict
        );
    }

    #[test]
    fn body_patch_applies_block_primitives() {
        let body = Body::new(vec![
            Block::new(
                "a",
                token("40000000000000000000000000000000"),
                BlockKind::Paragraph,
                vec![TextRun::new("text", vec![]).unwrap()],
                vec![],
            )
            .unwrap(),
        ]);
        let child = Block::new(
            "b",
            token("80000000000000000000000000000000"),
            BlockKind::Paragraph,
            vec![TextRun::new("child", vec![]).unwrap()],
            vec![],
        )
        .unwrap();
        let patch = BodyPatch::new(
            1,
            vec![
                BodyDelta::InsertBlock {
                    parent_id: Some("a".to_string()),
                    block: child,
                },
                BodyDelta::MoveBlock {
                    block_id: "b".to_string(),
                    new_parent_id: None,
                    new_order: token("20000000000000000000000000000000"),
                },
            ],
        )
        .unwrap();
        let updated = body.apply_patch(&patch, 1).unwrap();
        assert_eq!(updated.blocks[0].block_id, "b");
        assert!(updated.find_block("a").unwrap().children.is_empty());
    }

    #[test]
    fn body_patch_splice_checks_utf8_boundaries() {
        let body = Body::new(vec![
            Block::new(
                "a",
                token("40000000000000000000000000000000"),
                BlockKind::Paragraph,
                vec![TextRun::new("éx", vec![]).unwrap()],
                vec![],
            )
            .unwrap(),
        ]);
        let bad = BodyPatch::new(
            1,
            vec![BodyDelta::SpliceText {
                block_id: "a".to_string(),
                start: 1,
                end: 2,
                replacement: vec![TextRun::new("e", vec![]).unwrap()],
            }],
        )
        .unwrap();
        assert!(body.apply_patch(&bad, 1).is_err());
        let good = BodyPatch::new(
            1,
            vec![BodyDelta::SpliceText {
                block_id: "a".to_string(),
                start: 0,
                end: 2,
                replacement: vec![TextRun::new("e", vec![]).unwrap()],
            }],
        )
        .unwrap();
        let updated = body.apply_patch(&good, 1).unwrap();
        assert_eq!(updated.find_block("a").unwrap().runs[0].text, "ex");
    }

    #[test]
    fn anchors_shift_or_stale_across_splices() {
        let anchor = BodyAnchor::new("a", 5, 9).unwrap();
        assert_eq!(anchor.map_splice("a", 1, 3, 5).start, 8);
        assert_eq!(anchor.map_splice("other", 1, 3, 5), anchor);
        assert!(anchor.map_splice("a", 6, 7, 2).stale);
    }

    #[test]
    fn body_epoch_records_crypto_shred_visibility() {
        let live = BodyEpoch::new("epoch-1", 10, 4, false).unwrap();
        let retired = BodyEpoch::new("epoch-2", 11, 0, true).unwrap();
        assert!(live.can_render());
        assert!(!retired.can_render());
    }
}
