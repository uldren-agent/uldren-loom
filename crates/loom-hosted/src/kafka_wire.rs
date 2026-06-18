use std::io;
use std::sync::Arc;

use kafka_protocol::ResponseError;
use kafka_protocol::messages::{
    AddOffsetsToTxnRequest, AddOffsetsToTxnResponse, AddPartitionsToTxnRequest,
    AddPartitionsToTxnResponse, ApiKey, ApiVersionsResponse, CreateTopicsRequest,
    CreateTopicsResponse, DeleteTopicsRequest, DeleteTopicsResponse, EndTxnRequest, EndTxnResponse,
    FetchRequest, FetchResponse, InitProducerIdRequest, InitProducerIdResponse, MetadataRequest,
    MetadataResponse, OffsetCommitRequest, OffsetCommitResponse, ProduceRequest, ProduceResponse,
    ProducerId, RequestHeader, ResponseHeader, SaslAuthenticateRequest, SaslAuthenticateResponse,
    SaslHandshakeRequest, SaslHandshakeResponse, TopicName, TxnOffsetCommitRequest,
    TxnOffsetCommitResponse,
    add_partitions_to_txn_response::{
        AddPartitionsToTxnPartitionResult, AddPartitionsToTxnTopicResult,
    },
    api_versions_response::ApiVersion,
    create_topics_response::CreatableTopicResult,
    delete_topics_response::DeletableTopicResult,
    fetch_response::{AbortedTransaction, FetchableTopicResponse, PartitionData},
    metadata_response::{MetadataResponseBroker, MetadataResponsePartition, MetadataResponseTopic},
    offset_commit_response::{OffsetCommitResponsePartition, OffsetCommitResponseTopic},
    produce_response::{PartitionProduceResponse, TopicProduceResponse},
    txn_offset_commit_response::{TxnOffsetCommitResponsePartition, TxnOffsetCommitResponseTopic},
};
use kafka_protocol::protocol::{Decodable, Encodable, decode_request_header_from_buffer};
#[cfg(all(test, feature = "integration-tests"))]
use kafka_protocol::records::Compression;
use kafka_protocol::records::{
    NO_PARTITION_LEADER_EPOCH, NO_PRODUCER_EPOCH, NO_PRODUCER_ID, NO_SEQUENCE, RecordBatchDecoder,
    RecordBatchEncoder, RecordEncodeOptions,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::data::{
    HostedKafkaAbortedTransaction, HostedKafkaPendingOffsetCommit, HostedKafkaProducerAppend,
    HostedKafkaTopicMetadata, HostedKafkaTransactionTopic,
};
use crate::{HostedAuth, HostedKernel};

const MAX_FRAME_BYTES: usize = 1_048_576;

pub async fn serve_kafka_tcp<F>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspace: String,
    shutdown: F,
) -> io::Result<()>
where
    F: std::future::Future<Output = ()>,
{
    let state = Arc::new(KafkaServerState { kernel, workspace });
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    let _ = KafkaConnection {
                        stream,
                        state,
                        auth: None,
                    }.run().await;
                });
            }
        }
    }
}

struct KafkaServerState {
    kernel: HostedKernel,
    workspace: String,
}

struct KafkaConnection<S> {
    stream: S,
    state: Arc<KafkaServerState>,
    auth: Option<HostedAuth>,
}

impl<S> KafkaConnection<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    async fn run(mut self) -> io::Result<()> {
        loop {
            let Some(frame) = read_frame(&mut self.stream).await? else {
                return Ok(());
            };
            let Some(response) = self.response_for_frame(&frame)? else {
                return Ok(());
            };
            write_frame(&mut self.stream, &response).await?;
        }
    }

    fn response_for_frame(&mut self, frame: &[u8]) -> io::Result<Option<Vec<u8>>> {
        let mut request = bytes::Bytes::copy_from_slice(frame);
        let header = match decode_request_header_from_buffer(&mut request) {
            Ok(header) => header,
            Err(_) => return Ok(None),
        };
        let api_key = match ApiKey::try_from(header.request_api_key) {
            Ok(api_key) => api_key,
            Err(_) => return Ok(None),
        };
        let version = clamp_version(api_key, header.request_api_version);
        let mut response = Vec::new();
        encode_response_header(&mut response, api_key, version, &header)?;
        match api_key {
            ApiKey::ApiVersions => api_versions_response().encode(&mut response, version),
            ApiKey::SaslHandshake => {
                self.handle_sasl_handshake(&mut request, &mut response, version)
            }
            ApiKey::SaslAuthenticate => {
                self.handle_sasl_authenticate(&mut request, &mut response, version)
            }
            ApiKey::Metadata => self.handle_metadata(&mut request, &mut response, version),
            ApiKey::CreateTopics => self.handle_create_topics(&mut request, &mut response, version),
            ApiKey::DeleteTopics => self.handle_delete_topics(&mut request, &mut response, version),
            ApiKey::Produce => self.handle_produce(&mut request, &mut response, version),
            ApiKey::Fetch => self.handle_fetch(&mut request, &mut response, version),
            ApiKey::OffsetCommit => self.handle_offset_commit(&mut request, &mut response, version),
            ApiKey::InitProducerId => {
                self.handle_init_producer_id(&mut request, &mut response, version)
            }
            ApiKey::AddPartitionsToTxn => {
                self.handle_add_partitions_to_txn(&mut request, &mut response, version)
            }
            ApiKey::AddOffsetsToTxn => {
                self.handle_add_offsets_to_txn(&mut request, &mut response, version)
            }
            ApiKey::EndTxn => self.handle_end_txn(&mut request, &mut response, version),
            ApiKey::TxnOffsetCommit => {
                self.handle_txn_offset_commit(&mut request, &mut response, version)
            }
            _ => return Ok(None),
        }
        .map_err(invalid_data)?;
        Ok(Some(response))
    }

    fn handle_sasl_handshake(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = SaslHandshakeRequest::decode(request, version)?;
        let error_code = if request.mechanism.as_str().eq_ignore_ascii_case("PLAIN") {
            0
        } else {
            ResponseError::UnsupportedSaslMechanism.code()
        };
        SaslHandshakeResponse::default()
            .with_error_code(error_code)
            .with_mechanisms(vec![kafka_str("PLAIN".to_string())])
            .encode(response, version)
    }

    fn handle_sasl_authenticate(
        &mut self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = SaslAuthenticateRequest::decode(request, version)?;
        let result = parse_sasl_plain(&request.auth_bytes)
            .and_then(|auth| self.state.kernel.read(&auth, |_| Ok(())).map(|_| auth));
        match result {
            Ok(auth) => {
                self.auth = Some(auth);
                SaslAuthenticateResponse::default()
                    .with_error_code(0)
                    .with_error_message(None)
                    .with_session_lifetime_ms(0)
                    .encode(response, version)
            }
            Err(_) => SaslAuthenticateResponse::default()
                .with_error_code(ResponseError::SaslAuthenticationFailed.code())
                .with_error_message(Some(kafka_str("authentication failed".to_string())))
                .with_session_lifetime_ms(0)
                .encode(response, version),
        }
    }

    fn handle_metadata(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = MetadataRequest::decode(request, version)?;
        let metadata = match self.current_auth() {
            Some(auth) => {
                metadata_response(&self.state.kernel, auth, &self.state.workspace, &request)
            }
            None => unauthorized_metadata_response(&self.state.workspace, &request),
        };
        metadata.encode(response, version)
    }

    fn handle_create_topics(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = CreateTopicsRequest::decode(request, version)?;
        let topics = request
            .topics
            .iter()
            .map(|topic| {
                let Some(auth) = self.current_auth() else {
                    return topic_result(
                        topic.name.as_str(),
                        ResponseError::ClusterAuthorizationFailed.code(),
                        "authentication required",
                    );
                };
                if let Err(err) = validate_kafka_topic_name(topic.name.as_str()) {
                    return topic_result(
                        topic.name.as_str(),
                        ResponseError::InvalidRequest.code(),
                        &err.to_string(),
                    );
                }
                if topic.num_partitions != 1 || !topic.assignments.is_empty() {
                    return topic_result(
                        topic.name.as_str(),
                        ResponseError::InvalidRequest.code(),
                        "exactly one implicit partition is source-backed",
                    );
                }
                if request.validate_only {
                    return topic_result(topic.name.as_str(), 0, "");
                }
                match self.state.kernel.data().kafka_topic_create(
                    auth,
                    &self.state.workspace,
                    topic.name.as_str(),
                    topic.num_partitions,
                ) {
                    Ok(Some(_)) => topic_result(topic.name.as_str(), 0, ""),
                    Ok(None) => topic_result(
                        topic.name.as_str(),
                        ResponseError::TopicAlreadyExists.code(),
                        "topic already exists",
                    ),
                    Err(err) => topic_result(
                        topic.name.as_str(),
                        kafka_error_code(err.code),
                        &err.message,
                    ),
                }
            })
            .collect();
        CreateTopicsResponse::default()
            .with_topics(topics)
            .encode(response, version)
    }

    fn handle_delete_topics(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = DeleteTopicsRequest::decode(request, version)?;
        let names: Vec<String> = if version >= 6 {
            request
                .topics
                .iter()
                .filter_map(|topic| topic.name.as_ref().map(topic_name_string))
                .collect()
        } else {
            request.topic_names.iter().map(topic_name_string).collect()
        };
        let responses = names
            .iter()
            .map(|name| {
                let Some(auth) = self.current_auth() else {
                    return delete_result(
                        name,
                        ResponseError::ClusterAuthorizationFailed.code(),
                        "authentication required",
                    );
                };
                if let Err(err) = validate_kafka_topic_name(name) {
                    return delete_result(
                        name,
                        ResponseError::InvalidRequest.code(),
                        &err.to_string(),
                    );
                }
                match self
                    .state
                    .kernel
                    .data()
                    .kafka_topic_delete(auth, &self.state.workspace, name)
                {
                    Ok(true) => delete_result(name, 0, ""),
                    Ok(false) => delete_result(
                        name,
                        ResponseError::UnknownTopicOrPartition.code(),
                        "topic does not exist",
                    ),
                    Err(err) => delete_result(name, kafka_error_code(err.code), &err.message),
                }
            })
            .collect();
        DeleteTopicsResponse::default()
            .with_responses(responses)
            .encode(response, version)
    }

    fn handle_produce(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = ProduceRequest::decode(request, version)?;
        let responses =
            request
                .topic_data
                .iter()
                .map(|topic| {
                    let name = topic.name.as_str();
                    let partition_responses = topic
                    .partition_data
                    .iter()
                    .map(|partition| {
                        let Some(auth) = self.current_auth() else {
                            return produce_partition_result(
                                partition.index,
                                ResponseError::ClusterAuthorizationFailed.code(),
                                -1,
                                "authentication required",
                            );
                        };
                        if let Err(err) = validate_kafka_topic_name(name) {
                            return produce_partition_result(
                                partition.index,
                                ResponseError::InvalidRequest.code(),
                                -1,
                                &err.to_string(),
                            );
                        }
                        let Some(records) = partition.records.as_ref() else {
                            return produce_partition_result(
                                partition.index,
                                ResponseError::InvalidRequest.code(),
                                -1,
                                "Kafka produce records are required",
                            );
                        };
                        let base_offset = match self.state.kernel.data().kafka_next_offset(
                            auth,
                            &self.state.workspace,
                            name,
                            partition.index,
                        ) {
                            Ok(offset) => offset,
                            Err(err) => {
                                return produce_partition_result(
                                    partition.index,
                                    kafka_error_code(err.code),
                                    -1,
                                    &err.message,
                                );
                            }
                        };
                        let normalized = match normalize_kafka_record_batches(
                            records.clone(),
                            match i64::try_from(base_offset) {
                                Ok(offset) => offset,
                                Err(_) => {
                                    return produce_partition_result(
                                        partition.index,
                                        ResponseError::InvalidRequest.code(),
                                        -1,
                                        "Kafka base offset is too large",
                                    );
                                }
                            },
                            request.transactional_id.as_ref().map(|id| id.as_str()),
                        ) {
                            Ok(records) => records,
                            Err(err) => {
                                return produce_partition_result(
                                    partition.index,
                                    ResponseError::InvalidRequest.code(),
                                    -1,
                                    &err.to_string(),
                                );
                            }
                        };
                        let produce_result = match request.transactional_id.as_ref() {
                            Some(transactional_id) => {
                                let Some(producer_append) = normalized.producer_append.as_ref()
                                else {
                                    return produce_partition_result(
                                        partition.index,
                                        ResponseError::InvalidRequest.code(),
                                        -1,
                                        "Kafka transactional produce requires producer metadata",
                                    );
                                };
                                self.state.kernel.data().kafka_produce_transactional_records(
                                    auth,
                                    &self.state.workspace,
                                    transactional_id.as_str(),
                                    name,
                                    partition.index,
                                    base_offset,
                                    &normalized.record_batches,
                                    producer_append,
                                )
                            }
                            None => self.state.kernel.data().kafka_produce_records(
                                auth,
                                &self.state.workspace,
                                name,
                                partition.index,
                                base_offset,
                                &normalized.record_batches,
                                normalized.producer_append.as_ref(),
                            ),
                        };
                        match produce_result {
                            Ok(result) => produce_partition_result(
                                partition.index,
                                0,
                                result.base_offset as i64,
                                "",
                            ),
                            Err(err) => produce_partition_result(
                                partition.index,
                                kafka_produce_error_code(err.code),
                                -1,
                                &err.message,
                            ),
                        }
                    })
                    .collect();
                    TopicProduceResponse::default()
                        .with_name(topic_name(name))
                        .with_partition_responses(partition_responses)
                })
                .collect();
        ProduceResponse::default()
            .with_responses(responses)
            .encode(response, version)
    }

    fn handle_fetch(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = FetchRequest::decode(request, version)?;
        let responses = request
            .topics
            .iter()
            .map(|topic| {
                let name = topic.topic.as_str();
                let partitions = topic
                    .partitions
                    .iter()
                    .map(|partition| {
                        let Some(auth) = self.current_auth() else {
                            return fetch_partition_result(
                                partition.partition,
                                ResponseError::ClusterAuthorizationFailed.code(),
                                0,
                                0,
                                Vec::new(),
                                Vec::new(),
                            );
                        };
                        if let Err(err) = validate_kafka_topic_name(name) {
                            return fetch_partition_result(
                                partition.partition,
                                ResponseError::InvalidRequest.code(),
                                0,
                                0,
                                Vec::new(),
                                err.to_string().into_bytes(),
                            );
                        }
                        if partition.fetch_offset < 0 {
                            return fetch_partition_result(
                                partition.partition,
                                ResponseError::InvalidRequest.code(),
                                0,
                                0,
                                Vec::new(),
                                Vec::new(),
                            );
                        }
                        if request.isolation_level != 0 && request.isolation_level != 1 {
                            return fetch_partition_result(
                                partition.partition,
                                ResponseError::InvalidRequest.code(),
                                0,
                                0,
                                Vec::new(),
                                Vec::new(),
                            );
                        }
                        let max_bytes = usize::try_from(partition.partition_max_bytes.max(0))
                            .unwrap_or(usize::MAX);
                        match self.state.kernel.data().kafka_fetch_records(
                            auth,
                            &self.state.workspace,
                            name,
                            partition.partition,
                            partition.fetch_offset as u64,
                            max_bytes,
                            request.isolation_level == 1,
                        ) {
                            Ok(result) => fetch_partition_result(
                                partition.partition,
                                0,
                                result.high_watermark as i64,
                                result.last_stable_offset as i64,
                                result.aborted_transactions,
                                result.records,
                            ),
                            Err(err) => fetch_partition_result(
                                partition.partition,
                                kafka_error_code(err.code),
                                0,
                                0,
                                Vec::new(),
                                Vec::new(),
                            ),
                        }
                    })
                    .collect();
                FetchableTopicResponse::default()
                    .with_topic(topic_name(name))
                    .with_partitions(partitions)
            })
            .collect();
        FetchResponse::default()
            .with_responses(responses)
            .encode(response, version)
    }

    fn handle_offset_commit(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = OffsetCommitRequest::decode(request, version)?;
        let group_id = request.group_id.as_str();
        let topics = request
            .topics
            .iter()
            .map(|topic| {
                let name = topic.name.as_str();
                let partitions = topic
                    .partitions
                    .iter()
                    .map(|partition| {
                        let Some(auth) = self.current_auth() else {
                            return offset_commit_partition_result(
                                partition.partition_index,
                                ResponseError::ClusterAuthorizationFailed.code(),
                            );
                        };
                        if validate_kafka_topic_name(name).is_err()
                            || group_id.is_empty()
                            || partition.committed_offset < 0
                        {
                            return offset_commit_partition_result(
                                partition.partition_index,
                                ResponseError::InvalidRequest.code(),
                            );
                        }
                        match self.state.kernel.data().kafka_offset_commit(
                            auth,
                            &self.state.workspace,
                            name,
                            partition.partition_index,
                            group_id,
                            partition.committed_offset as u64,
                        ) {
                            Ok(()) => offset_commit_partition_result(partition.partition_index, 0),
                            Err(err) => offset_commit_partition_result(
                                partition.partition_index,
                                kafka_error_code(err.code),
                            ),
                        }
                    })
                    .collect();
                OffsetCommitResponseTopic::default()
                    .with_name(topic_name(name))
                    .with_partitions(partitions)
            })
            .collect();
        OffsetCommitResponse::default()
            .with_topics(topics)
            .encode(response, version)
    }

    fn handle_init_producer_id(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = InitProducerIdRequest::decode(request, version)?;
        let Some(auth) = self.current_auth() else {
            return InitProducerIdResponse::default()
                .with_error_code(ResponseError::ClusterAuthorizationFailed.code())
                .with_producer_id(ProducerId(-1))
                .with_producer_epoch(-1)
                .encode(response, version);
        };
        let transactional_id = request
            .transactional_id
            .as_ref()
            .map(|value| value.as_str());
        let result = self.state.kernel.data().kafka_init_producer_id(
            auth,
            &self.state.workspace,
            transactional_id,
            *request.producer_id,
            request.producer_epoch,
        );
        match result {
            Ok(state) => InitProducerIdResponse::default()
                .with_error_code(0)
                .with_producer_id(ProducerId(state.producer_id))
                .with_producer_epoch(state.producer_epoch)
                .encode(response, version),
            Err(err) => InitProducerIdResponse::default()
                .with_error_code(kafka_error_code(err.code))
                .with_producer_id(ProducerId(-1))
                .with_producer_epoch(-1)
                .encode(response, version),
        }
    }

    fn handle_add_partitions_to_txn(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = AddPartitionsToTxnRequest::decode(request, version)?;
        let transactional_id = request.v3_and_below_transactional_id.as_str();
        let topics: Vec<_> = request
            .v3_and_below_topics
            .iter()
            .map(|topic| HostedKafkaTransactionTopic {
                topic: topic.name.as_str().to_string(),
                partitions: topic.partitions.clone(),
            })
            .collect();
        let code = match self.current_auth() {
            Some(auth) => {
                let invalid_topic = topics
                    .iter()
                    .any(|topic| validate_kafka_topic_name(&topic.topic).is_err());
                if invalid_topic {
                    ResponseError::InvalidRequest.code()
                } else {
                    match self
                        .state
                        .kernel
                        .data()
                        .kafka_add_partitions_to_transaction(
                            auth,
                            &self.state.workspace,
                            transactional_id,
                            *request.v3_and_below_producer_id,
                            request.v3_and_below_producer_epoch,
                            &topics,
                        ) {
                        Ok(()) => 0,
                        Err(err) => kafka_error_code(err.code),
                    }
                }
            }
            None => ResponseError::ClusterAuthorizationFailed.code(),
        };
        let topic_results = topics
            .into_iter()
            .map(|topic| {
                let partition_results = topic
                    .partitions
                    .into_iter()
                    .map(|partition| {
                        AddPartitionsToTxnPartitionResult::default()
                            .with_partition_index(partition)
                            .with_partition_error_code(code)
                    })
                    .collect();
                AddPartitionsToTxnTopicResult::default()
                    .with_name(topic_name(&topic.topic))
                    .with_results_by_partition(partition_results)
            })
            .collect();
        AddPartitionsToTxnResponse::default()
            .with_results_by_topic_v3_and_below(topic_results)
            .encode(response, version)
    }

    fn handle_add_offsets_to_txn(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = AddOffsetsToTxnRequest::decode(request, version)?;
        let code = match self.current_auth() {
            Some(auth) => match self.state.kernel.data().kafka_add_offsets_to_transaction(
                auth,
                &self.state.workspace,
                request.transactional_id.as_str(),
                *request.producer_id,
                request.producer_epoch,
                request.group_id.as_str(),
            ) {
                Ok(()) => 0,
                Err(err) => kafka_error_code(err.code),
            },
            None => ResponseError::ClusterAuthorizationFailed.code(),
        };
        AddOffsetsToTxnResponse::default()
            .with_error_code(code)
            .encode(response, version)
    }

    fn handle_end_txn(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = EndTxnRequest::decode(request, version)?;
        let code = match self.current_auth() {
            Some(auth) => match self.state.kernel.data().kafka_end_transaction(
                auth,
                &self.state.workspace,
                request.transactional_id.as_str(),
                *request.producer_id,
                request.producer_epoch,
                request.committed,
            ) {
                Ok(()) => 0,
                Err(err) => kafka_error_code(err.code),
            },
            None => ResponseError::ClusterAuthorizationFailed.code(),
        };
        EndTxnResponse::default()
            .with_error_code(code)
            .with_producer_id(request.producer_id)
            .with_producer_epoch(request.producer_epoch)
            .encode(response, version)
    }

    fn handle_txn_offset_commit(
        &self,
        request: &mut bytes::Bytes,
        response: &mut Vec<u8>,
        version: i16,
    ) -> anyhow::Result<()> {
        let request = TxnOffsetCommitRequest::decode(request, version)?;
        let offsets = request
            .topics
            .iter()
            .flat_map(|topic| {
                topic.partitions.iter().map(|partition| {
                    (
                        topic.name.as_str(),
                        partition.partition_index,
                        partition.committed_offset,
                    )
                })
            })
            .collect::<Vec<_>>();
        let validation_code = match self.current_auth() {
            Some(auth) => {
                let invalid_offset = offsets.iter().any(|(topic, _, offset)| {
                    validate_kafka_topic_name(topic).is_err() || *offset < 0
                });
                if invalid_offset {
                    ResponseError::InvalidRequest.code()
                } else {
                    let pending = offsets
                        .iter()
                        .map(
                            |(topic, partition, offset)| HostedKafkaPendingOffsetCommit {
                                group_id: request.group_id.as_str().to_string(),
                                topic: (*topic).to_string(),
                                partition: *partition,
                                offset: *offset as u64,
                            },
                        )
                        .collect::<Vec<_>>();
                    match self.state.kernel.data().kafka_txn_offset_commit(
                        auth,
                        &self.state.workspace,
                        request.transactional_id.as_str(),
                        *request.producer_id,
                        request.producer_epoch,
                        &pending,
                    ) {
                        Ok(()) => 0,
                        Err(err) => kafka_error_code(err.code),
                    }
                }
            }
            None => ResponseError::ClusterAuthorizationFailed.code(),
        };
        let topics = request
            .topics
            .iter()
            .map(|topic| {
                let partitions = topic
                    .partitions
                    .iter()
                    .map(|partition| {
                        TxnOffsetCommitResponsePartition::default()
                            .with_partition_index(partition.partition_index)
                            .with_error_code(validation_code)
                    })
                    .collect();
                TxnOffsetCommitResponseTopic::default()
                    .with_name(topic_name(topic.name.as_str()))
                    .with_partitions(partitions)
            })
            .collect();
        TxnOffsetCommitResponse::default()
            .with_topics(topics)
            .encode(response, version)
    }

    fn current_auth(&self) -> Option<&HostedAuth> {
        self.auth.as_ref()
    }
}

fn encode_response_header(
    out: &mut Vec<u8>,
    api_key: ApiKey,
    version: i16,
    header: &RequestHeader,
) -> io::Result<()> {
    ResponseHeader::default()
        .with_correlation_id(header.correlation_id)
        .encode(out, api_key.response_header_version(version))
        .map_err(invalid_data)
}

fn api_versions_response() -> ApiVersionsResponse {
    ApiVersionsResponse::default().with_api_keys(vec![
        ApiVersion::default()
            .with_api_key(ApiKey::SaslHandshake as i16)
            .with_min_version(supported_min(ApiKey::SaslHandshake))
            .with_max_version(supported_max(ApiKey::SaslHandshake)),
        ApiVersion::default()
            .with_api_key(ApiKey::SaslAuthenticate as i16)
            .with_min_version(supported_min(ApiKey::SaslAuthenticate))
            .with_max_version(supported_max(ApiKey::SaslAuthenticate)),
        ApiVersion::default()
            .with_api_key(ApiKey::Metadata as i16)
            .with_min_version(supported_min(ApiKey::Metadata))
            .with_max_version(supported_max(ApiKey::Metadata)),
        ApiVersion::default()
            .with_api_key(ApiKey::CreateTopics as i16)
            .with_min_version(supported_min(ApiKey::CreateTopics))
            .with_max_version(supported_max(ApiKey::CreateTopics)),
        ApiVersion::default()
            .with_api_key(ApiKey::DeleteTopics as i16)
            .with_min_version(supported_min(ApiKey::DeleteTopics))
            .with_max_version(supported_max(ApiKey::DeleteTopics)),
        ApiVersion::default()
            .with_api_key(ApiKey::Produce as i16)
            .with_min_version(supported_min(ApiKey::Produce))
            .with_max_version(supported_max(ApiKey::Produce)),
        ApiVersion::default()
            .with_api_key(ApiKey::Fetch as i16)
            .with_min_version(supported_min(ApiKey::Fetch))
            .with_max_version(supported_max(ApiKey::Fetch)),
        ApiVersion::default()
            .with_api_key(ApiKey::OffsetCommit as i16)
            .with_min_version(supported_min(ApiKey::OffsetCommit))
            .with_max_version(supported_max(ApiKey::OffsetCommit)),
        ApiVersion::default()
            .with_api_key(ApiKey::InitProducerId as i16)
            .with_min_version(supported_min(ApiKey::InitProducerId))
            .with_max_version(supported_max(ApiKey::InitProducerId)),
        ApiVersion::default()
            .with_api_key(ApiKey::AddPartitionsToTxn as i16)
            .with_min_version(supported_min(ApiKey::AddPartitionsToTxn))
            .with_max_version(supported_max(ApiKey::AddPartitionsToTxn)),
        ApiVersion::default()
            .with_api_key(ApiKey::AddOffsetsToTxn as i16)
            .with_min_version(supported_min(ApiKey::AddOffsetsToTxn))
            .with_max_version(supported_max(ApiKey::AddOffsetsToTxn)),
        ApiVersion::default()
            .with_api_key(ApiKey::EndTxn as i16)
            .with_min_version(supported_min(ApiKey::EndTxn))
            .with_max_version(supported_max(ApiKey::EndTxn)),
        ApiVersion::default()
            .with_api_key(ApiKey::TxnOffsetCommit as i16)
            .with_min_version(supported_min(ApiKey::TxnOffsetCommit))
            .with_max_version(supported_max(ApiKey::TxnOffsetCommit)),
        ApiVersion::default()
            .with_api_key(ApiKey::ApiVersions as i16)
            .with_min_version(supported_min(ApiKey::ApiVersions))
            .with_max_version(supported_max(ApiKey::ApiVersions)),
    ])
}

fn metadata_response(
    kernel: &HostedKernel,
    auth: &HostedAuth,
    workspace: &str,
    request: &MetadataRequest,
) -> MetadataResponse {
    let requested = requested_topics(request);
    let topic_names = match requested {
        Some(names) => names,
        None => kernel
            .data()
            .kafka_topics(auth, workspace)
            .map(|topics| topics.into_iter().map(|topic| topic.topic).collect())
            .unwrap_or_default(),
    };
    MetadataResponse::default()
        .with_brokers(vec![
            MetadataResponseBroker::default()
                .with_node_id(0.into())
                .with_host(kafka_str("localhost".to_string()))
                .with_port(0),
        ])
        .with_cluster_id(Some(kafka_str(format!("loom-{workspace}"))))
        .with_controller_id(0.into())
        .with_topics(
            topic_names
                .into_iter()
                .map(|name| metadata_topic(kernel, auth, workspace, &name))
                .collect(),
        )
}

fn unauthorized_metadata_response(workspace: &str, request: &MetadataRequest) -> MetadataResponse {
    MetadataResponse::default()
        .with_cluster_id(Some(kafka_str(format!("loom-{workspace}"))))
        .with_controller_id(0.into())
        .with_topics(
            requested_topics(request)
                .unwrap_or_default()
                .into_iter()
                .map(|name| {
                    MetadataResponseTopic::default()
                        .with_error_code(ResponseError::TopicAuthorizationFailed.code())
                        .with_name(Some(topic_name(&name)))
                })
                .collect(),
        )
}

fn metadata_topic(
    kernel: &HostedKernel,
    auth: &HostedAuth,
    workspace: &str,
    name: &str,
) -> MetadataResponseTopic {
    if validate_kafka_topic_name(name).is_err() {
        return MetadataResponseTopic::default()
            .with_error_code(ResponseError::InvalidRequest.code())
            .with_name(Some(topic_name(name)));
    }
    match kernel.data().kafka_topic_metadata(auth, workspace, name) {
        Ok(metadata) => metadata_topic_from_record(&metadata),
        Err(err) => MetadataResponseTopic::default()
            .with_error_code(kafka_error_code(err.code))
            .with_name(Some(topic_name(name))),
    }
}

fn metadata_topic_from_record(metadata: &HostedKafkaTopicMetadata) -> MetadataResponseTopic {
    MetadataResponseTopic::default()
        .with_error_code(0)
        .with_name(Some(topic_name(&metadata.topic)))
        .with_topic_id(uuid::Uuid::from_bytes(metadata.topic_id))
        .with_partitions(
            metadata
                .partitions
                .iter()
                .map(|partition| {
                    MetadataResponsePartition::default()
                        .with_partition_index(partition.partition)
                        .with_leader_id(partition.leader_id.into())
                        .with_leader_epoch(partition.leader_epoch as i32)
                        .with_replica_nodes(vec![partition.leader_id.into()])
                        .with_isr_nodes(vec![partition.leader_id.into()])
                })
                .collect(),
        )
}

fn requested_topics(request: &MetadataRequest) -> Option<Vec<String>> {
    request.topics.as_ref().map(|topics| {
        topics
            .iter()
            .filter_map(|topic| topic.name.as_ref().map(topic_name_string))
            .collect()
    })
}

fn topic_result(name: &str, code: i16, message: &str) -> CreatableTopicResult {
    CreatableTopicResult::default()
        .with_name(topic_name(name))
        .with_error_code(code)
        .with_error_message(error_message(code, message))
        .with_num_partitions(if code == 0 { 1 } else { -1 })
        .with_replication_factor(if code == 0 { 1 } else { -1 })
}

fn delete_result(name: &str, code: i16, message: &str) -> DeletableTopicResult {
    DeletableTopicResult::default()
        .with_name(Some(topic_name(name)))
        .with_error_code(code)
        .with_error_message(error_message(code, message))
}

fn produce_partition_result(
    partition: i32,
    code: i16,
    base_offset: i64,
    message: &str,
) -> PartitionProduceResponse {
    PartitionProduceResponse::default()
        .with_index(partition)
        .with_error_code(code)
        .with_base_offset(base_offset)
        .with_log_append_time_ms(-1)
        .with_log_start_offset(0)
        .with_error_message(error_message(code, message))
}

fn fetch_partition_result(
    partition: i32,
    code: i16,
    high_watermark: i64,
    last_stable_offset: i64,
    aborted_transactions: Vec<HostedKafkaAbortedTransaction>,
    records: Vec<u8>,
) -> PartitionData {
    PartitionData::default()
        .with_partition_index(partition)
        .with_error_code(code)
        .with_high_watermark(high_watermark)
        .with_last_stable_offset(last_stable_offset)
        .with_log_start_offset(0)
        .with_aborted_transactions(Some(
            aborted_transactions
                .into_iter()
                .map(|entry| {
                    AbortedTransaction::default()
                        .with_producer_id(ProducerId(entry.producer_id))
                        .with_first_offset(entry.first_offset as i64)
                })
                .collect(),
        ))
        .with_records(Some(bytes::Bytes::from(records)))
}

struct NormalizedKafkaRecords {
    record_batches: Vec<Vec<u8>>,
    producer_append: Option<HostedKafkaProducerAppend>,
}

fn normalize_kafka_record_batches(
    mut records: bytes::Bytes,
    base_offset: i64,
    transactional_id: Option<&str>,
) -> anyhow::Result<NormalizedKafkaRecords> {
    let decoded = RecordBatchDecoder::decode_all(&mut records)?;
    let mut normalized = Vec::new();
    let mut producer_append: Option<HostedKafkaProducerAppend> = None;
    let mut saw_ordinary_record = false;
    let mut next_offset = base_offset;
    for record_set in decoded {
        if record_set.version != 2 {
            anyhow::bail!(
                "Kafka record batch version {} is unsupported",
                record_set.version
            );
        }
        for mut record in record_set.records {
            update_kafka_producer_append(
                &mut producer_append,
                &mut saw_ordinary_record,
                &record,
                transactional_id.is_some(),
            )?;
            record.offset = next_offset;
            let mut batch = bytes::BytesMut::new();
            RecordBatchEncoder::encode(
                &mut batch,
                std::iter::once(&record),
                &RecordEncodeOptions {
                    version: 2,
                    compression: record_set.compression,
                },
            )?;
            normalized.push(batch.to_vec());
            next_offset = next_offset
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Kafka offset overflows"))?;
        }
    }
    if normalized.is_empty() {
        anyhow::bail!("Kafka produce records are empty");
    }
    Ok(NormalizedKafkaRecords {
        record_batches: normalized,
        producer_append,
    })
}

fn update_kafka_producer_append(
    producer_append: &mut Option<HostedKafkaProducerAppend>,
    saw_ordinary_record: &mut bool,
    record: &kafka_protocol::records::Record,
    allow_transactional: bool,
) -> anyhow::Result<()> {
    if record.transactional && !allow_transactional {
        anyhow::bail!("Kafka transactional records require a transactional id");
    }
    if allow_transactional && !record.transactional {
        anyhow::bail!("Kafka transactional produce requires transactional records");
    }
    let has_producer = record.producer_id != NO_PRODUCER_ID
        || record.producer_epoch != NO_PRODUCER_EPOCH
        || record.sequence != NO_SEQUENCE;
    if !has_producer {
        if producer_append.is_some() {
            anyhow::bail!("Kafka producer-aware and ordinary records cannot be mixed");
        }
        *saw_ordinary_record = true;
        return Ok(());
    }
    if *saw_ordinary_record {
        anyhow::bail!("Kafka producer-aware and ordinary records cannot be mixed");
    }
    if record.producer_id == NO_PRODUCER_ID
        || record.producer_epoch == NO_PRODUCER_EPOCH
        || record.sequence == NO_SEQUENCE
    {
        anyhow::bail!("Kafka producer metadata is incomplete");
    }
    if record.partition_leader_epoch != NO_PARTITION_LEADER_EPOCH
        && record.partition_leader_epoch < 0
    {
        anyhow::bail!("Kafka partition leader epoch is invalid");
    }
    match producer_append {
        Some(append) => {
            if append.producer_id != record.producer_id
                || append.producer_epoch != record.producer_epoch
            {
                anyhow::bail!("Kafka producer metadata changed within one append");
            }
            let expected_sequence = append
                .first_sequence
                .checked_add(i32::try_from(append.record_count)?)
                .ok_or_else(|| anyhow::anyhow!("Kafka producer sequence overflows"))?;
            if record.sequence != expected_sequence {
                anyhow::bail!("Kafka producer sequence is not contiguous");
            }
            append.record_count = append
                .record_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Kafka producer record count overflows"))?;
        }
        None => {
            *producer_append = Some(HostedKafkaProducerAppend {
                producer_id: record.producer_id,
                producer_epoch: record.producer_epoch,
                first_sequence: record.sequence,
                record_count: 1,
            });
        }
    }
    Ok(())
}

fn offset_commit_partition_result(partition: i32, code: i16) -> OffsetCommitResponsePartition {
    OffsetCommitResponsePartition::default()
        .with_partition_index(partition)
        .with_error_code(code)
}

fn error_message(code: i16, message: &str) -> Option<kafka_protocol::protocol::StrBytes> {
    (code != 0).then(|| kafka_str(message.to_string()))
}

fn kafka_error_code(code: loom_core::Code) -> i16 {
    match code {
        loom_core::Code::NotFound => ResponseError::UnknownTopicOrPartition.code(),
        loom_core::Code::PermissionDenied => ResponseError::TopicAuthorizationFailed.code(),
        loom_core::Code::AuthenticationFailed => ResponseError::SaslAuthenticationFailed.code(),
        loom_core::Code::InvalidArgument => ResponseError::InvalidRequest.code(),
        loom_core::Code::AlreadyExists => ResponseError::TopicAlreadyExists.code(),
        loom_core::Code::FencingStale => ResponseError::InvalidProducerEpoch.code(),
        loom_core::Code::Conflict => ResponseError::InvalidTxnState.code(),
        loom_core::Code::Unsupported => ResponseError::UnsupportedForMessageFormat.code(),
        _ => ResponseError::UnknownServerError.code(),
    }
}

fn kafka_produce_error_code(code: loom_core::Code) -> i16 {
    match code {
        loom_core::Code::Conflict => ResponseError::OutOfOrderSequenceNumber.code(),
        _ => kafka_error_code(code),
    }
}

fn parse_sasl_plain(bytes: &[u8]) -> loom_core::Result<HostedAuth> {
    let mut parts = bytes.split(|byte| *byte == 0);
    let _authzid = parts.next();
    let Some(username) = parts
        .next()
        .and_then(|value| std::str::from_utf8(value).ok())
    else {
        return Err(loom_core::LoomError::invalid("invalid SASL PLAIN username"));
    };
    let Some(password) = parts
        .next()
        .and_then(|value| std::str::from_utf8(value).ok())
    else {
        return Err(loom_core::LoomError::invalid("invalid SASL PLAIN password"));
    };
    if parts.next().is_some() {
        return Err(loom_core::LoomError::invalid(
            "invalid SASL PLAIN field count",
        ));
    }
    if password.starts_with("loom_app_") {
        return Ok(HostedAuth::app_credential(
            password,
            format!("kafka-sasl-plain-app:{username}"),
        ));
    }
    let principal = loom_core::WorkspaceId::parse(username)?;
    Ok(HostedAuth::passphrase(
        principal,
        password,
        format!("kafka-sasl-plain:{username}"),
    ))
}

fn clamp_version(api_key: ApiKey, requested: i16) -> i16 {
    requested.clamp(supported_min(api_key), supported_max(api_key))
}

fn supported_min(api_key: ApiKey) -> i16 {
    match api_key {
        ApiKey::ApiVersions => 0,
        ApiKey::Produce => 3,
        ApiKey::Fetch => 4,
        ApiKey::OffsetCommit => 2,
        ApiKey::InitProducerId => 0,
        ApiKey::AddPartitionsToTxn => 0,
        ApiKey::AddOffsetsToTxn => 0,
        ApiKey::EndTxn => 0,
        ApiKey::TxnOffsetCommit => 0,
        ApiKey::Metadata => 0,
        _ => api_key.valid_versions().min,
    }
}

fn supported_max(api_key: ApiKey) -> i16 {
    match api_key {
        ApiKey::ApiVersions => 4,
        ApiKey::Metadata => 13,
        ApiKey::Produce => 12,
        ApiKey::Fetch => 12,
        ApiKey::OffsetCommit => 9,
        ApiKey::InitProducerId => 5,
        ApiKey::AddPartitionsToTxn => 3,
        ApiKey::AddOffsetsToTxn => 4,
        ApiKey::EndTxn => 5,
        ApiKey::TxnOffsetCommit => 5,
        _ => api_key.valid_versions().max,
    }
}

async fn read_frame<S>(stream: &mut S) -> io::Result<Option<Vec<u8>>>
where
    S: AsyncRead + Unpin,
{
    let mut len = [0_u8; 4];
    match stream.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let frame_len = i32::from_be_bytes(len);
    if frame_len < 0 || frame_len as usize > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid Kafka frame length",
        ));
    }
    let mut frame = vec![0_u8; frame_len as usize];
    stream.read_exact(&mut frame).await?;
    Ok(Some(frame))
}

async fn write_frame<S>(stream: &mut S, response: &[u8]) -> io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let len = i32::try_from(response.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Kafka response frame exceeds protocol length",
        )
    })?;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(response).await?;
    stream.flush().await
}

fn kafka_str(value: String) -> kafka_protocol::protocol::StrBytes {
    kafka_protocol::protocol::StrBytes::from_string(value)
}

fn topic_name(value: &str) -> TopicName {
    TopicName(kafka_str(value.to_string()))
}

fn topic_name_string(value: &TopicName) -> String {
    value.as_str().to_string()
}

fn validate_kafka_topic_name(value: &str) -> loom_core::Result<()> {
    if value.is_empty() {
        return Err(loom_core::LoomError::invalid("Kafka topic name is empty"));
    }
    if value.len() > 249 {
        return Err(loom_core::LoomError::invalid(
            "Kafka topic name is too long",
        ));
    }
    if value == "." || value == ".." {
        return Err(loom_core::LoomError::invalid(
            "Kafka topic name is reserved",
        ));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(loom_core::LoomError::invalid(
            "Kafka topic name contains unsupported characters",
        ));
    }
    Ok(())
}

fn invalid_data<E>(err: E) -> io::Error
where
    E: std::fmt::Display,
{
    io::Error::new(io::ErrorKind::InvalidData, err.to_string())
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use bytes::BytesMut;
    use kafka_protocol::messages::{
        AddOffsetsToTxnRequest, AddPartitionsToTxnRequest, ApiVersionsRequest, CreateTopicsRequest,
        DeleteTopicsRequest, EndTxnRequest, InitProducerIdRequest, MetadataRequest,
        OffsetCommitRequest, ProduceRequest, SaslAuthenticateRequest, SaslHandshakeRequest,
        TxnOffsetCommitRequest,
        add_partitions_to_txn_request::AddPartitionsToTxnTopic,
        create_topics_request::CreatableTopic,
        delete_topics_request::DeleteTopicState,
        fetch_request::{FetchPartition, FetchTopic},
        offset_commit_request::{OffsetCommitRequestPartition, OffsetCommitRequestTopic},
        produce_request::{PartitionProduceData, TopicProduceData},
        txn_offset_commit_request::{TxnOffsetCommitRequestPartition, TxnOffsetCommitRequestTopic},
    };
    use kafka_protocol::protocol::{Decodable, Encodable, encode_request_header_into_buffer};
    use kafka_protocol::records::{Record, TimestampType};

    use super::*;
    use crate::test_support::{init, nid, temp_path};

    #[test]
    fn api_versions_response_reports_only_source_backed_apis() {
        let path = temp_path("kafka-api-versions");
        init(&path, None);
        let mut connection = test_connection(&path, "work");
        let response = connection
            .response_for_frame(&request_frame(
                ApiKey::ApiVersions,
                4,
                ApiVersionsRequest::default(),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(response, ApiKey::ApiVersions, 4);
        let response = ApiVersionsResponse::decode(&mut decoded, 4).unwrap();
        let api_keys: Vec<i16> = response.api_keys.iter().map(|key| key.api_key).collect();

        assert_eq!(
            api_keys,
            vec![
                ApiKey::SaslHandshake as i16,
                ApiKey::SaslAuthenticate as i16,
                ApiKey::Metadata as i16,
                ApiKey::CreateTopics as i16,
                ApiKey::DeleteTopics as i16,
                ApiKey::Produce as i16,
                ApiKey::Fetch as i16,
                ApiKey::OffsetCommit as i16,
                ApiKey::InitProducerId as i16,
                ApiKey::AddPartitionsToTxn as i16,
                ApiKey::AddOffsetsToTxn as i16,
                ApiKey::EndTxn as i16,
                ApiKey::TxnOffsetCommit as i16,
                ApiKey::ApiVersions as i16,
            ]
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn metadata_requires_authentication() {
        let path = temp_path("kafka-metadata-auth");
        init(&path, None);
        let mut connection = test_connection(&path, "work");
        let response = connection
            .response_for_frame(&request_frame(
                ApiKey::Metadata,
                13,
                MetadataRequest::default().with_topics(Some(vec![
                    kafka_protocol::messages::metadata_request::MetadataRequestTopic::default()
                        .with_name(Some(topic_name("events"))),
                ])),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(response, ApiKey::Metadata, 13);
        let response = MetadataResponse::decode(&mut decoded, 13).unwrap();

        assert_eq!(
            response.topics[0].error_code,
            ResponseError::TopicAuthorizationFailed.code()
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn sasl_plain_create_topic_and_metadata_are_source_backed() {
        let path = temp_path("kafka-create-metadata");
        init(&path, None);
        let mut connection = test_connection(&path, "work");
        let handshake = connection
            .response_for_frame(&request_frame(
                ApiKey::SaslHandshake,
                1,
                SaslHandshakeRequest::default().with_mechanism(kafka_str("PLAIN".to_string())),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(handshake, ApiKey::SaslHandshake, 1);
        let handshake = SaslHandshakeResponse::decode(&mut decoded, 1).unwrap();
        assert_eq!(handshake.error_code, 0);

        let auth_bytes = format!("\0{}\0root-pass", nid(1));
        let auth = connection
            .response_for_frame(&request_frame(
                ApiKey::SaslAuthenticate,
                2,
                SaslAuthenticateRequest::default().with_auth_bytes(bytes::Bytes::from(auth_bytes)),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(auth, ApiKey::SaslAuthenticate, 2);
        let auth = SaslAuthenticateResponse::decode(&mut decoded, 2).unwrap();
        assert_eq!(auth.error_code, 0);

        let create = connection
            .response_for_frame(&request_frame(
                ApiKey::CreateTopics,
                7,
                CreateTopicsRequest::default().with_topics(vec![
                    CreatableTopic::default()
                        .with_name(topic_name("events"))
                        .with_num_partitions(1)
                        .with_replication_factor(1),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(create, ApiKey::CreateTopics, 7);
        let create = CreateTopicsResponse::decode(&mut decoded, 7).unwrap();
        assert_eq!(create.topics[0].error_code, 0);

        let metadata = connection
            .response_for_frame(&request_frame(
                ApiKey::Metadata,
                13,
                MetadataRequest::default().with_topics(Some(vec![
                    kafka_protocol::messages::metadata_request::MetadataRequestTopic::default()
                        .with_name(Some(topic_name("events"))),
                ])),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(metadata, ApiKey::Metadata, 13);
        let metadata = MetadataResponse::decode(&mut decoded, 13).unwrap();
        assert_eq!(metadata.cluster_id.as_ref().unwrap().as_str(), "loom-work");
        assert_eq!(metadata.topics[0].error_code, 0);
        assert_eq!(metadata.topics[0].name.as_ref().unwrap().as_str(), "events");
        assert_eq!(metadata.topics[0].partitions.len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn topic_metadata_persists_and_delete_removes_it() {
        let path = temp_path("kafka-topic-metadata");
        init(&path, None);
        let mut connection = authenticated_connection(&path, "work");

        let create = connection
            .response_for_frame(&request_frame(
                ApiKey::CreateTopics,
                7,
                CreateTopicsRequest::default().with_topics(vec![
                    CreatableTopic::default()
                        .with_name(topic_name("events"))
                        .with_num_partitions(1)
                        .with_replication_factor(1),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(create, ApiKey::CreateTopics, 7);
        let create = CreateTopicsResponse::decode(&mut decoded, 7).unwrap();
        assert_eq!(create.topics[0].error_code, 0);

        let mut connection = authenticated_connection(&path, "work");
        let metadata = connection
            .response_for_frame(&request_frame(
                ApiKey::Metadata,
                13,
                MetadataRequest::default().with_topics(None),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(metadata, ApiKey::Metadata, 13);
        let metadata = MetadataResponse::decode(&mut decoded, 13).unwrap();
        assert_eq!(metadata.topics.len(), 1);
        assert_eq!(metadata.topics[0].name.as_ref().unwrap().as_str(), "events");
        assert_ne!(metadata.topics[0].topic_id, uuid::Uuid::nil());
        assert_eq!(metadata.topics[0].partitions[0].leader_epoch, 1);

        let delete = connection
            .response_for_frame(&request_frame(
                ApiKey::DeleteTopics,
                6,
                DeleteTopicsRequest::default().with_topics(vec![
                    DeleteTopicState::default().with_name(Some(topic_name("events"))),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(delete, ApiKey::DeleteTopics, 6);
        let delete = DeleteTopicsResponse::decode(&mut decoded, 6).unwrap();
        assert_eq!(delete.responses[0].error_code, 0);

        let metadata = connection
            .response_for_frame(&request_frame(
                ApiKey::Metadata,
                13,
                MetadataRequest::default().with_topics(None),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(metadata, ApiKey::Metadata, 13);
        let metadata = MetadataResponse::decode(&mut decoded, 13).unwrap();
        assert!(metadata.topics.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn produce_fetch_and_offset_commit_are_queue_backed() {
        let path = temp_path("kafka-produce-fetch");
        init(&path, None);
        let mut connection = authenticated_connection(&path, "work");
        create_topic(&mut connection, "events");

        let records = record_batch(&[b"one".as_slice(), b"two".as_slice()]);
        let produce = connection
            .response_for_frame(&request_frame(
                ApiKey::Produce,
                12,
                ProduceRequest::default().with_acks(1).with_topic_data(vec![
                    TopicProduceData::default()
                        .with_name(topic_name("events"))
                        .with_partition_data(vec![
                            PartitionProduceData::default()
                                .with_index(0)
                                .with_records(Some(records.clone())),
                        ]),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(produce, ApiKey::Produce, 12);
        let produce = ProduceResponse::decode(&mut decoded, 12).unwrap();
        let partition = &produce.responses[0].partition_responses[0];
        assert_eq!(partition.error_code, 0);
        assert_eq!(partition.base_offset, 0);

        let fetch = connection
            .response_for_frame(&request_frame(
                ApiKey::Fetch,
                12,
                FetchRequest::default().with_topics(vec![
                    FetchTopic::default()
                        .with_topic(topic_name("events"))
                        .with_partitions(vec![
                            FetchPartition::default()
                                .with_partition(0)
                                .with_fetch_offset(0)
                                .with_partition_max_bytes(1024),
                        ]),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(fetch, ApiKey::Fetch, 12);
        let fetch = FetchResponse::decode(&mut decoded, 12).unwrap();
        let partition = &fetch.responses[0].partitions[0];
        assert_eq!(partition.error_code, 0);
        assert_eq!(partition.high_watermark, 2);
        let fetched = decoded_records(partition.records.clone().unwrap());
        assert_eq!(
            fetched
                .iter()
                .map(|record| record.offset)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(
            fetched
                .iter()
                .map(|record| record.value.as_ref().unwrap().as_ref())
                .collect::<Vec<_>>(),
            vec![b"one".as_slice(), b"two".as_slice()]
        );

        let fetch_from_second = connection
            .response_for_frame(&request_frame(
                ApiKey::Fetch,
                12,
                FetchRequest::default().with_topics(vec![
                    FetchTopic::default()
                        .with_topic(topic_name("events"))
                        .with_partitions(vec![
                            FetchPartition::default()
                                .with_partition(0)
                                .with_fetch_offset(1)
                                .with_partition_max_bytes(1024),
                        ]),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(fetch_from_second, ApiKey::Fetch, 12);
        let fetch = FetchResponse::decode(&mut decoded, 12).unwrap();
        let partition = &fetch.responses[0].partitions[0];
        assert_eq!(partition.error_code, 0);
        assert_eq!(partition.high_watermark, 2);
        let fetched = decoded_records(partition.records.clone().unwrap());
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].offset, 1);
        assert_eq!(fetched[0].value.as_ref().unwrap().as_ref(), b"two");

        let commit = connection
            .response_for_frame(&request_frame(
                ApiKey::OffsetCommit,
                9,
                OffsetCommitRequest::default()
                    .with_group_id(kafka_protocol::messages::GroupId(
                        kafka_protocol::protocol::StrBytes::from_static_str("workers"),
                    ))
                    .with_member_id(kafka_protocol::protocol::StrBytes::from_static_str(
                        "member-a",
                    ))
                    .with_topics(vec![
                        OffsetCommitRequestTopic::default()
                            .with_name(topic_name("events"))
                            .with_partitions(vec![
                                OffsetCommitRequestPartition::default()
                                    .with_partition_index(0)
                                    .with_committed_offset(2),
                            ]),
                    ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(commit, ApiKey::OffsetCommit, 9);
        let commit = OffsetCommitResponse::decode(&mut decoded, 9).unwrap();
        assert_eq!(commit.topics[0].partitions[0].error_code, 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn kafka_metadata_versions_use_shared_durable_coordination_sequence() {
        let path = temp_path("kafka-metadata-version-sequence");
        init(&path, None);
        let mut connection = authenticated_connection(&path, "work");
        create_topic(&mut connection, "events");

        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "kafka-metadata-version-test");
        let events = kernel
            .data()
            .kafka_topic_metadata(&auth, "work", "events")
            .unwrap();
        assert_eq!(events.metadata_version, 1);

        let records = record_batch(&[b"one".as_slice()]);
        let produce = connection
            .response_for_frame(&request_frame(
                ApiKey::Produce,
                12,
                ProduceRequest::default().with_acks(1).with_topic_data(vec![
                    TopicProduceData::default()
                        .with_name(topic_name("events"))
                        .with_partition_data(vec![
                            PartitionProduceData::default()
                                .with_index(0)
                                .with_records(Some(records)),
                        ]),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(produce, ApiKey::Produce, 12);
        let produce = ProduceResponse::decode(&mut decoded, 12).unwrap();
        assert_eq!(
            produce.responses[0].partition_responses[0].error_code, 0,
            "{:?}",
            produce.responses[0].partition_responses[0].error_message
        );

        let events = kernel
            .data()
            .kafka_topic_metadata(&auth, "work", "events")
            .unwrap();
        assert_eq!(events.metadata_version, 2);

        create_topic(&mut connection, "audit");
        let audit = kernel
            .data()
            .kafka_topic_metadata(&auth, "work", "audit")
            .unwrap();
        assert_eq!(audit.metadata_version, 3);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn idempotent_produce_validates_sequences_and_recognizes_duplicate_retry() {
        let path = temp_path("kafka-idempotent-produce");
        init(&path, None);
        let mut connection = authenticated_connection(&path, "work");
        create_topic(&mut connection, "events");

        let init = connection
            .response_for_frame(&request_frame(
                ApiKey::InitProducerId,
                5,
                InitProducerIdRequest::default()
                    .with_transactional_id(None)
                    .with_producer_id(ProducerId(-1))
                    .with_producer_epoch(-1),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(init, ApiKey::InitProducerId, 5);
        let init = InitProducerIdResponse::decode(&mut decoded, 5).unwrap();
        assert_eq!(init.error_code, 0);

        let first_records = record_batch_with_producer(
            &[b"one".as_slice(), b"two".as_slice()],
            init.producer_id.0,
            init.producer_epoch,
            0,
        );
        let produce = connection
            .response_for_frame(&request_frame(
                ApiKey::Produce,
                12,
                ProduceRequest::default().with_acks(1).with_topic_data(vec![
                    TopicProduceData::default()
                        .with_name(topic_name("events"))
                        .with_partition_data(vec![
                            PartitionProduceData::default()
                                .with_index(0)
                                .with_records(Some(first_records.clone())),
                        ]),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(produce, ApiKey::Produce, 12);
        let produce = ProduceResponse::decode(&mut decoded, 12).unwrap();
        assert_eq!(produce.responses[0].partition_responses[0].error_code, 0);
        assert_eq!(produce.responses[0].partition_responses[0].base_offset, 0);

        let duplicate = connection
            .response_for_frame(&request_frame(
                ApiKey::Produce,
                12,
                ProduceRequest::default().with_acks(1).with_topic_data(vec![
                    TopicProduceData::default()
                        .with_name(topic_name("events"))
                        .with_partition_data(vec![
                            PartitionProduceData::default()
                                .with_index(0)
                                .with_records(Some(first_records)),
                        ]),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(duplicate, ApiKey::Produce, 12);
        let duplicate = ProduceResponse::decode(&mut decoded, 12).unwrap();
        assert_eq!(duplicate.responses[0].partition_responses[0].error_code, 0);
        assert_eq!(duplicate.responses[0].partition_responses[0].base_offset, 0);

        let out_of_order = connection
            .response_for_frame(&request_frame(
                ApiKey::Produce,
                12,
                ProduceRequest::default().with_acks(1).with_topic_data(vec![
                    TopicProduceData::default()
                        .with_name(topic_name("events"))
                        .with_partition_data(vec![
                            PartitionProduceData::default()
                                .with_index(0)
                                .with_records(Some(record_batch_with_producer(
                                    &[b"gap".as_slice()],
                                    init.producer_id.0,
                                    init.producer_epoch,
                                    4,
                                ))),
                        ]),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(out_of_order, ApiKey::Produce, 12);
        let out_of_order = ProduceResponse::decode(&mut decoded, 12).unwrap();
        assert_eq!(
            out_of_order.responses[0].partition_responses[0].error_code,
            ResponseError::OutOfOrderSequenceNumber.code()
        );

        let fetch = connection
            .response_for_frame(&request_frame(
                ApiKey::Fetch,
                12,
                FetchRequest::default().with_topics(vec![
                    FetchTopic::default()
                        .with_topic(topic_name("events"))
                        .with_partitions(vec![
                            FetchPartition::default()
                                .with_partition(0)
                                .with_fetch_offset(0)
                                .with_partition_max_bytes(1024),
                        ]),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(fetch, ApiKey::Fetch, 12);
        let fetch = FetchResponse::decode(&mut decoded, 12).unwrap();
        assert_eq!(fetch.responses[0].partitions[0].high_watermark, 2);
        let fetched = decoded_records(fetch.responses[0].partitions[0].records.clone().unwrap());
        assert_eq!(fetched.len(), 2);
        assert_eq!(fetched[0].sequence, 0);
        assert_eq!(fetched[1].sequence, 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn produce_rejects_invalid_record_batch_bytes() {
        let path = temp_path("kafka-invalid-record-batch");
        init(&path, None);
        let mut connection = authenticated_connection(&path, "work");
        create_topic(&mut connection, "events");

        let produce = connection
            .response_for_frame(&request_frame(
                ApiKey::Produce,
                12,
                ProduceRequest::default().with_acks(1).with_topic_data(vec![
                    TopicProduceData::default()
                        .with_name(topic_name("events"))
                        .with_partition_data(vec![
                            PartitionProduceData::default()
                                .with_index(0)
                                .with_records(Some(bytes::Bytes::from_static(
                                    b"opaque-record-batch",
                                ))),
                        ]),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(produce, ApiKey::Produce, 12);
        let produce = ProduceResponse::decode(&mut decoded, 12).unwrap();
        let partition = &produce.responses[0].partition_responses[0];
        assert_eq!(partition.error_code, ResponseError::InvalidRequest.code());
        assert_eq!(partition.base_offset, -1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn produce_accepts_compressed_record_batches() {
        for compression in [
            Compression::Gzip,
            Compression::Snappy,
            Compression::Lz4,
            Compression::Zstd,
        ] {
            let path = temp_path(&format!("kafka-compressed-{compression:?}"));
            init(&path, None);
            let mut connection = authenticated_connection(&path, "work");
            create_topic(&mut connection, "events");

            let records = record_batch_with_compression(&[b"compressed".as_slice()], compression);
            let produce = connection
                .response_for_frame(&request_frame(
                    ApiKey::Produce,
                    12,
                    ProduceRequest::default().with_acks(1).with_topic_data(vec![
                        TopicProduceData::default()
                            .with_name(topic_name("events"))
                            .with_partition_data(vec![
                                PartitionProduceData::default()
                                    .with_index(0)
                                    .with_records(Some(records)),
                            ]),
                    ]),
                ))
                .unwrap()
                .unwrap();
            let mut decoded = response_body(produce, ApiKey::Produce, 12);
            let produce = ProduceResponse::decode(&mut decoded, 12).unwrap();
            let partition = &produce.responses[0].partition_responses[0];
            assert_eq!(partition.error_code, 0);
            assert_eq!(partition.base_offset, 0);

            let fetch = connection
                .response_for_frame(&request_frame(
                    ApiKey::Fetch,
                    12,
                    FetchRequest::default().with_topics(vec![
                        FetchTopic::default()
                            .with_topic(topic_name("events"))
                            .with_partitions(vec![
                                FetchPartition::default()
                                    .with_partition(0)
                                    .with_fetch_offset(0)
                                    .with_partition_max_bytes(1024),
                            ]),
                    ]),
                ))
                .unwrap()
                .unwrap();
            let mut decoded = response_body(fetch, ApiKey::Fetch, 12);
            let fetch = FetchResponse::decode(&mut decoded, 12).unwrap();
            let partition = &fetch.responses[0].partitions[0];
            assert_eq!(partition.error_code, 0);
            assert_eq!(partition.high_watermark, 1);
            let fetched = decoded_records(partition.records.clone().unwrap());
            assert_eq!(fetched.len(), 1);
            assert_eq!(fetched[0].offset, 0);
            assert_eq!(fetched[0].value.as_ref().unwrap().as_ref(), b"compressed");
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn producer_epoch_and_transaction_control_are_source_backed() {
        let path = temp_path("kafka-transaction-control");
        init(&path, None);
        let mut connection = authenticated_connection(&path, "work");
        create_topic(&mut connection, "events");

        let init = connection
            .response_for_frame(&request_frame(
                ApiKey::InitProducerId,
                5,
                InitProducerIdRequest::default()
                    .with_transactional_id(Some(transactional_id("tx-a")))
                    .with_producer_id(ProducerId(-1))
                    .with_producer_epoch(-1),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(init, ApiKey::InitProducerId, 5);
        let init = InitProducerIdResponse::decode(&mut decoded, 5).unwrap();
        assert_eq!(init.error_code, 0);
        assert!(init.producer_id.0 > 0);
        assert_eq!(init.producer_epoch, 0);

        let bumped = connection
            .response_for_frame(&request_frame(
                ApiKey::InitProducerId,
                5,
                InitProducerIdRequest::default()
                    .with_transactional_id(Some(transactional_id("tx-a")))
                    .with_producer_id(init.producer_id)
                    .with_producer_epoch(init.producer_epoch),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(bumped, ApiKey::InitProducerId, 5);
        let bumped = InitProducerIdResponse::decode(&mut decoded, 5).unwrap();
        assert_eq!(bumped.error_code, 0);
        assert_eq!(bumped.producer_id, init.producer_id);
        assert_eq!(bumped.producer_epoch, 1);

        let stale_end = connection
            .response_for_frame(&request_frame(
                ApiKey::EndTxn,
                5,
                EndTxnRequest::default()
                    .with_transactional_id(transactional_id("tx-a"))
                    .with_producer_id(init.producer_id)
                    .with_producer_epoch(init.producer_epoch)
                    .with_committed(true),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(stale_end, ApiKey::EndTxn, 5);
        let stale_end = EndTxnResponse::decode(&mut decoded, 5).unwrap();
        assert_eq!(
            stale_end.error_code,
            ResponseError::InvalidProducerEpoch.code()
        );

        let add_partitions = connection
            .response_for_frame(&request_frame(
                ApiKey::AddPartitionsToTxn,
                3,
                AddPartitionsToTxnRequest::default()
                    .with_v3_and_below_transactional_id(transactional_id("tx-a"))
                    .with_v3_and_below_producer_id(bumped.producer_id)
                    .with_v3_and_below_producer_epoch(bumped.producer_epoch)
                    .with_v3_and_below_topics(vec![
                        AddPartitionsToTxnTopic::default()
                            .with_name(topic_name("events"))
                            .with_partitions(vec![0]),
                    ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(add_partitions, ApiKey::AddPartitionsToTxn, 3);
        let add_partitions = AddPartitionsToTxnResponse::decode(&mut decoded, 3).unwrap();
        assert_eq!(
            add_partitions.results_by_topic_v3_and_below[0].results_by_partition[0]
                .partition_error_code,
            0
        );

        let add_offsets = connection
            .response_for_frame(&request_frame(
                ApiKey::AddOffsetsToTxn,
                4,
                AddOffsetsToTxnRequest::default()
                    .with_transactional_id(transactional_id("tx-a"))
                    .with_producer_id(bumped.producer_id)
                    .with_producer_epoch(bumped.producer_epoch)
                    .with_group_id(kafka_protocol::messages::GroupId(
                        kafka_protocol::protocol::StrBytes::from_static_str("workers"),
                    )),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(add_offsets, ApiKey::AddOffsetsToTxn, 4);
        let add_offsets = AddOffsetsToTxnResponse::decode(&mut decoded, 4).unwrap();
        assert_eq!(add_offsets.error_code, 0);

        let seed = connection
            .response_for_frame(&request_frame(
                ApiKey::Produce,
                12,
                ProduceRequest::default().with_acks(1).with_topic_data(vec![
                    TopicProduceData::default()
                        .with_name(topic_name("events"))
                        .with_partition_data(vec![
                            PartitionProduceData::default()
                                .with_index(0)
                                .with_records(Some(record_batch(&[b"seed".as_slice()]))),
                        ]),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(seed, ApiKey::Produce, 12);
        let seed = ProduceResponse::decode(&mut decoded, 12).unwrap();
        assert_eq!(seed.responses[0].partition_responses[0].error_code, 0);

        let txn_offset_commit = connection
            .response_for_frame(&request_frame(
                ApiKey::TxnOffsetCommit,
                5,
                TxnOffsetCommitRequest::default()
                    .with_transactional_id(transactional_id("tx-a"))
                    .with_group_id(kafka_protocol::messages::GroupId(
                        kafka_protocol::protocol::StrBytes::from_static_str("workers"),
                    ))
                    .with_producer_id(bumped.producer_id)
                    .with_producer_epoch(bumped.producer_epoch)
                    .with_member_id(kafka_protocol::protocol::StrBytes::from_static_str(
                        "member-a",
                    ))
                    .with_topics(vec![
                        TxnOffsetCommitRequestTopic::default()
                            .with_name(topic_name("events"))
                            .with_partitions(vec![
                                TxnOffsetCommitRequestPartition::default()
                                    .with_partition_index(0)
                                    .with_committed_offset(1),
                            ]),
                    ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(txn_offset_commit, ApiKey::TxnOffsetCommit, 5);
        let txn_offset_commit = TxnOffsetCommitResponse::decode(&mut decoded, 5).unwrap();
        assert_eq!(txn_offset_commit.topics[0].partitions[0].error_code, 0);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "kafka-txn-offset-test");
        assert_eq!(
            kernel
                .data()
                .kafka_offset_position(&auth, "work", "events", 0, "workers")
                .unwrap(),
            0
        );

        let end = connection
            .response_for_frame(&request_frame(
                ApiKey::EndTxn,
                5,
                EndTxnRequest::default()
                    .with_transactional_id(transactional_id("tx-a"))
                    .with_producer_id(bumped.producer_id)
                    .with_producer_epoch(bumped.producer_epoch)
                    .with_committed(true),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(end, ApiKey::EndTxn, 5);
        let end = EndTxnResponse::decode(&mut decoded, 5).unwrap();
        assert_eq!(end.error_code, 0);
        assert_eq!(
            kernel
                .data()
                .kafka_offset_position(&auth, "work", "events", 0, "workers")
                .unwrap(),
            1
        );

        let end_again = connection
            .response_for_frame(&request_frame(
                ApiKey::EndTxn,
                5,
                EndTxnRequest::default()
                    .with_transactional_id(transactional_id("tx-a"))
                    .with_producer_id(bumped.producer_id)
                    .with_producer_epoch(bumped.producer_epoch)
                    .with_committed(false),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(end_again, ApiKey::EndTxn, 5);
        let end_again = EndTxnResponse::decode(&mut decoded, 5).unwrap();
        assert_eq!(end_again.error_code, ResponseError::InvalidTxnState.code());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn transactional_produce_visibility_is_source_backed() {
        let path = temp_path("kafka-transactional-produce-visibility");
        init(&path, None);
        let mut connection = authenticated_connection(&path, "work");
        create_topic(&mut connection, "events");

        let init_a = connection
            .response_for_frame(&request_frame(
                ApiKey::InitProducerId,
                5,
                InitProducerIdRequest::default()
                    .with_transactional_id(Some(transactional_id("tx-a")))
                    .with_producer_id(ProducerId(-1))
                    .with_producer_epoch(-1),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(init_a, ApiKey::InitProducerId, 5);
        let init_a = InitProducerIdResponse::decode(&mut decoded, 5).unwrap();
        assert_eq!(init_a.error_code, 0);

        add_transaction_partition(
            &mut connection,
            "tx-a",
            init_a.producer_id,
            init_a.producer_epoch,
        );

        let produce = connection
            .response_for_frame(&request_frame(
                ApiKey::Produce,
                12,
                ProduceRequest::default()
                    .with_transactional_id(Some(transactional_id("tx-a")))
                    .with_acks(1)
                    .with_topic_data(vec![
                        TopicProduceData::default()
                            .with_name(topic_name("events"))
                            .with_partition_data(vec![
                                PartitionProduceData::default()
                                    .with_index(0)
                                    .with_records(Some(transactional_record_batch(
                                        init_a.producer_id.0,
                                        init_a.producer_epoch,
                                        0,
                                        b"committed",
                                    ))),
                            ]),
                    ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(produce, ApiKey::Produce, 12);
        let produce = ProduceResponse::decode(&mut decoded, 12).unwrap();
        let partition = &produce.responses[0].partition_responses[0];
        assert_eq!(partition.error_code, 0);
        assert_eq!(partition.base_offset, 0);

        let fetch_uncommitted = fetch_records(&mut connection, 0, 0);
        assert_eq!(fetch_uncommitted.high_watermark, 1);
        let records = decoded_records(fetch_uncommitted.records.clone().unwrap());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].value.as_ref().unwrap().as_ref(), b"committed");

        let fetch_committed = fetch_records(&mut connection, 1, 0);
        assert_eq!(fetch_committed.high_watermark, 1);
        assert_eq!(fetch_committed.last_stable_offset, 0);
        assert!(decoded_records(fetch_committed.records.clone().unwrap()).is_empty());

        let end_a = connection
            .response_for_frame(&request_frame(
                ApiKey::EndTxn,
                5,
                EndTxnRequest::default()
                    .with_transactional_id(transactional_id("tx-a"))
                    .with_producer_id(init_a.producer_id)
                    .with_producer_epoch(init_a.producer_epoch)
                    .with_committed(true),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(end_a, ApiKey::EndTxn, 5);
        let end_a = EndTxnResponse::decode(&mut decoded, 5).unwrap();
        assert_eq!(end_a.error_code, 0);

        let fetch_committed = fetch_records(&mut connection, 1, 0);
        assert_eq!(fetch_committed.high_watermark, 1);
        assert_eq!(fetch_committed.last_stable_offset, 1);
        let records = decoded_records(fetch_committed.records.clone().unwrap());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].value.as_ref().unwrap().as_ref(), b"committed");

        let init_b = connection
            .response_for_frame(&request_frame(
                ApiKey::InitProducerId,
                5,
                InitProducerIdRequest::default()
                    .with_transactional_id(Some(transactional_id("tx-b")))
                    .with_producer_id(ProducerId(-1))
                    .with_producer_epoch(-1),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(init_b, ApiKey::InitProducerId, 5);
        let init_b = InitProducerIdResponse::decode(&mut decoded, 5).unwrap();
        assert_eq!(init_b.error_code, 0);
        add_transaction_partition(
            &mut connection,
            "tx-b",
            init_b.producer_id,
            init_b.producer_epoch,
        );

        let produce_aborted = connection
            .response_for_frame(&request_frame(
                ApiKey::Produce,
                12,
                ProduceRequest::default()
                    .with_transactional_id(Some(transactional_id("tx-b")))
                    .with_acks(1)
                    .with_topic_data(vec![
                        TopicProduceData::default()
                            .with_name(topic_name("events"))
                            .with_partition_data(vec![
                                PartitionProduceData::default()
                                    .with_index(0)
                                    .with_records(Some(transactional_record_batch(
                                        init_b.producer_id.0,
                                        init_b.producer_epoch,
                                        0,
                                        b"aborted",
                                    ))),
                            ]),
                    ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(produce_aborted, ApiKey::Produce, 12);
        let produce_aborted = ProduceResponse::decode(&mut decoded, 12).unwrap();
        assert_eq!(
            produce_aborted.responses[0].partition_responses[0].error_code,
            0
        );
        assert_eq!(
            produce_aborted.responses[0].partition_responses[0].base_offset,
            1
        );

        let end_b = connection
            .response_for_frame(&request_frame(
                ApiKey::EndTxn,
                5,
                EndTxnRequest::default()
                    .with_transactional_id(transactional_id("tx-b"))
                    .with_producer_id(init_b.producer_id)
                    .with_producer_epoch(init_b.producer_epoch)
                    .with_committed(false),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(end_b, ApiKey::EndTxn, 5);
        let end_b = EndTxnResponse::decode(&mut decoded, 5).unwrap();
        assert_eq!(end_b.error_code, 0);

        let fetch_committed = fetch_records(&mut connection, 1, 0);
        assert_eq!(fetch_committed.high_watermark, 2);
        assert_eq!(fetch_committed.last_stable_offset, 2);
        assert_eq!(
            fetch_committed.aborted_transactions.as_ref().unwrap().len(),
            1
        );
        assert_eq!(
            fetch_committed.aborted_transactions.as_ref().unwrap()[0].first_offset,
            1
        );
        let records = decoded_records(fetch_committed.records.clone().unwrap());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].value.as_ref().unwrap().as_ref(), b"committed");
        std::fs::remove_file(path).unwrap();
    }

    fn request_frame<T>(api_key: ApiKey, version: i16, request: T) -> Vec<u8>
    where
        T: Encodable,
    {
        let mut frame = BytesMut::new();
        let header = RequestHeader::default()
            .with_request_api_key(api_key as i16)
            .with_request_api_version(version)
            .with_correlation_id(7)
            .with_client_id(Some(kafka_str("test-client".to_string())));
        encode_request_header_into_buffer(&mut frame, &header).unwrap();
        request.encode(&mut frame, version).unwrap();
        frame.to_vec()
    }

    fn response_body(response: Vec<u8>, api_key: ApiKey, version: i16) -> bytes::Bytes {
        let mut response = bytes::Bytes::from(response);
        let header_version = api_key.response_header_version(version);
        let header = ResponseHeader::decode(&mut response, header_version).unwrap();
        assert_eq!(header.correlation_id, 7);
        response
    }

    fn record_batch(values: &[&[u8]]) -> bytes::Bytes {
        record_batch_with_compression(values, Compression::None)
    }

    fn transactional_record_batch(
        producer_id: i64,
        producer_epoch: i16,
        sequence: i32,
        value: &'static [u8],
    ) -> bytes::Bytes {
        let record = Record {
            transactional: true,
            control: false,
            partition_leader_epoch: 0,
            producer_id,
            producer_epoch,
            timestamp_type: TimestampType::Creation,
            offset: 0,
            sequence,
            timestamp: 1_700_000_000_000,
            key: None,
            value: Some(bytes::Bytes::from_static(value)),
            headers: Default::default(),
        };
        let mut batch = BytesMut::new();
        RecordBatchEncoder::encode(
            &mut batch,
            std::iter::once(&record),
            &RecordEncodeOptions {
                version: 2,
                compression: Compression::None,
            },
        )
        .unwrap();
        batch.freeze()
    }

    fn record_batch_with_producer(
        values: &[&[u8]],
        producer_id: i64,
        producer_epoch: i16,
        first_sequence: i32,
    ) -> bytes::Bytes {
        let records: Vec<Record> = values
            .iter()
            .enumerate()
            .map(|(index, value)| Record {
                transactional: false,
                control: false,
                partition_leader_epoch: 0,
                producer_id,
                producer_epoch,
                timestamp_type: TimestampType::Creation,
                offset: 80 + index as i64,
                sequence: first_sequence + index as i32,
                timestamp: 1_700_000_000_000 + index as i64,
                key: None,
                value: Some(bytes::Bytes::copy_from_slice(value)),
                headers: Default::default(),
            })
            .collect();
        let mut batch = BytesMut::new();
        RecordBatchEncoder::encode(
            &mut batch,
            records.iter(),
            &RecordEncodeOptions {
                version: 2,
                compression: Compression::None,
            },
        )
        .unwrap();
        batch.freeze()
    }

    fn record_batch_with_compression(values: &[&[u8]], compression: Compression) -> bytes::Bytes {
        let records: Vec<Record> = values
            .iter()
            .enumerate()
            .map(|(index, value)| Record {
                transactional: false,
                control: false,
                partition_leader_epoch: 0,
                producer_id: -1,
                producer_epoch: -1,
                timestamp_type: TimestampType::Creation,
                offset: 40 + index as i64,
                sequence: -1,
                timestamp: 1_700_000_000_000 + index as i64,
                key: None,
                value: Some(bytes::Bytes::copy_from_slice(value)),
                headers: Default::default(),
            })
            .collect();
        let mut batch = BytesMut::new();
        RecordBatchEncoder::encode(
            &mut batch,
            records.iter(),
            &RecordEncodeOptions {
                version: 2,
                compression,
            },
        )
        .unwrap();
        batch.freeze()
    }

    fn transactional_id(value: &str) -> kafka_protocol::messages::TransactionalId {
        kafka_protocol::messages::TransactionalId(kafka_str(value.to_string()))
    }

    fn decoded_records(mut records: bytes::Bytes) -> Vec<Record> {
        RecordBatchDecoder::decode_all(&mut records)
            .unwrap()
            .into_iter()
            .flat_map(|set| set.records)
            .collect()
    }

    fn test_connection(
        path: &std::path::Path,
        workspace: &str,
    ) -> KafkaConnection<tokio::io::DuplexStream> {
        let (_client, server) = tokio::io::duplex(1024);
        KafkaConnection {
            stream: server,
            state: Arc::new(KafkaServerState {
                kernel: HostedKernel::new(path),
                workspace: workspace.to_string(),
            }),
            auth: None,
        }
    }

    fn authenticated_connection(
        path: &std::path::Path,
        workspace: &str,
    ) -> KafkaConnection<tokio::io::DuplexStream> {
        let mut connection = test_connection(path, workspace);
        let auth_bytes = format!("\0{}\0root-pass", nid(1));
        let response = connection
            .response_for_frame(&request_frame(
                ApiKey::SaslAuthenticate,
                2,
                SaslAuthenticateRequest::default().with_auth_bytes(bytes::Bytes::from(auth_bytes)),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(response, ApiKey::SaslAuthenticate, 2);
        let auth = SaslAuthenticateResponse::decode(&mut decoded, 2).unwrap();
        assert_eq!(auth.error_code, 0);
        connection
    }

    fn create_topic(connection: &mut KafkaConnection<tokio::io::DuplexStream>, name: &str) {
        let create = connection
            .response_for_frame(&request_frame(
                ApiKey::CreateTopics,
                7,
                CreateTopicsRequest::default().with_topics(vec![
                    CreatableTopic::default()
                        .with_name(topic_name(name))
                        .with_num_partitions(1)
                        .with_replication_factor(1),
                ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(create, ApiKey::CreateTopics, 7);
        let create = CreateTopicsResponse::decode(&mut decoded, 7).unwrap();
        assert_eq!(create.topics[0].error_code, 0);
    }

    fn add_transaction_partition(
        connection: &mut KafkaConnection<tokio::io::DuplexStream>,
        transactional_id_value: &str,
        producer_id: ProducerId,
        producer_epoch: i16,
    ) {
        let add_partitions = connection
            .response_for_frame(&request_frame(
                ApiKey::AddPartitionsToTxn,
                3,
                AddPartitionsToTxnRequest::default()
                    .with_v3_and_below_transactional_id(transactional_id(transactional_id_value))
                    .with_v3_and_below_producer_id(producer_id)
                    .with_v3_and_below_producer_epoch(producer_epoch)
                    .with_v3_and_below_topics(vec![
                        AddPartitionsToTxnTopic::default()
                            .with_name(topic_name("events"))
                            .with_partitions(vec![0]),
                    ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(add_partitions, ApiKey::AddPartitionsToTxn, 3);
        let add_partitions = AddPartitionsToTxnResponse::decode(&mut decoded, 3).unwrap();
        assert_eq!(
            add_partitions.results_by_topic_v3_and_below[0].results_by_partition[0]
                .partition_error_code,
            0
        );
    }

    fn fetch_records(
        connection: &mut KafkaConnection<tokio::io::DuplexStream>,
        isolation_level: i8,
        fetch_offset: i64,
    ) -> PartitionData {
        let fetch = connection
            .response_for_frame(&request_frame(
                ApiKey::Fetch,
                12,
                FetchRequest::default()
                    .with_isolation_level(isolation_level)
                    .with_topics(vec![
                        FetchTopic::default()
                            .with_topic(topic_name("events"))
                            .with_partitions(vec![
                                FetchPartition::default()
                                    .with_partition(0)
                                    .with_fetch_offset(fetch_offset)
                                    .with_partition_max_bytes(1024),
                            ]),
                    ]),
            ))
            .unwrap()
            .unwrap();
        let mut decoded = response_body(fetch, ApiKey::Fetch, 12);
        let fetch = FetchResponse::decode(&mut decoded, 12).unwrap();
        let partition = fetch.responses[0].partitions[0].clone();
        assert_eq!(partition.error_code, 0);
        partition
    }
}
