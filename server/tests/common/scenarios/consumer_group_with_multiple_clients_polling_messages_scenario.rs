use crate::common::{ClientFactory, TestServer};
use iggy::client::{ConsumerGroupClient, MessageClient, StreamClient, SystemClient, TopicClient};
use iggy::clients::client::{IggyClient, IggyClientConfig};
use iggy::consumer_groups::create_consumer_group::CreateConsumerGroup;
use iggy::consumer_groups::get_consumer_group::GetConsumerGroup;
use iggy::consumer_groups::join_consumer_group::JoinConsumerGroup;
use iggy::consumer_type::ConsumerType;
use iggy::identifier::Identifier;
use iggy::messages::poll_messages::Kind::Next;
use iggy::messages::poll_messages::{Format, PollMessages};
use iggy::messages::send_messages::{Key, Message, SendMessages};
use iggy::models::consumer_group::ConsumerGroupDetails;
use iggy::streams::create_stream::CreateStream;
use iggy::streams::delete_stream::DeleteStream;
use iggy::system::get_me::GetMe;
use iggy::topics::create_topic::CreateTopic;
use std::str::{from_utf8, FromStr};
use tokio::time::sleep;

const STREAM_ID: u32 = 1;
const TOPIC_ID: u32 = 1;
const STREAM_NAME: &str = "test-stream";
const TOPIC_NAME: &str = "test-topic";
const PARTITIONS_COUNT: u32 = 3;
const CONSUMER_GROUP_ID: u32 = 1;
const MESSAGES_COUNT: u32 = 1000;

#[allow(dead_code)]
pub async fn run(client_factory: &dyn ClientFactory) {
    let test_server = TestServer::default();
    test_server.start();
    sleep(std::time::Duration::from_secs(1)).await;
    let system_client = create_client(client_factory).await;
    let client1 = create_client(client_factory).await;
    let client2 = create_client(client_factory).await;
    let client3 = create_client(client_factory).await;

    init_system(&system_client, &client1, &client2, &client3).await;
    execute_using_entity_id_key(&system_client, &client1, &client2, &client3).await;
    system_client
        .delete_stream(&DeleteStream {
            stream_id: STREAM_ID,
        })
        .await
        .unwrap();
    init_system(&system_client, &client1, &client2, &client3).await;
    execute_using_none_key(&system_client, &client1, &client2, &client3).await;
    test_server.stop();
}

async fn create_client(client_factory: &dyn ClientFactory) -> IggyClient {
    let client = client_factory.create_client().await;
    IggyClient::new(client, IggyClientConfig::default())
}

async fn init_system(
    system_client: &IggyClient,
    client1: &IggyClient,
    client2: &IggyClient,
    client3: &IggyClient,
) {
    // 1. Create the stream
    let create_stream = CreateStream {
        stream_id: STREAM_ID,
        name: STREAM_NAME.to_string(),
    };
    system_client.create_stream(&create_stream).await.unwrap();

    // 2. Create the topic
    let create_topic = CreateTopic {
        stream_id: STREAM_ID,
        topic_id: TOPIC_ID,
        partitions_count: PARTITIONS_COUNT,
        name: TOPIC_NAME.to_string(),
    };
    system_client.create_topic(&create_topic).await.unwrap();

    // 3. Create the consumer group
    let create_group = CreateConsumerGroup {
        stream_id: STREAM_ID,
        topic_id: TOPIC_ID,
        consumer_group_id: CONSUMER_GROUP_ID,
    };
    system_client
        .create_consumer_group(&create_group)
        .await
        .unwrap();

    let join_group = JoinConsumerGroup {
        stream_id: STREAM_ID,
        topic_id: TOPIC_ID,
        consumer_group_id: CONSUMER_GROUP_ID,
    };

    // 4. Join the consumer group by each client
    client1.join_consumer_group(&join_group).await.unwrap();
    client2.join_consumer_group(&join_group).await.unwrap();
    client3.join_consumer_group(&join_group).await.unwrap();

    // 5. Get the consumer group details
    let consumer_group_info = system_client
        .get_consumer_group(&GetConsumerGroup {
            stream_id: STREAM_ID,
            topic_id: TOPIC_ID,
            consumer_group_id: CONSUMER_GROUP_ID,
        })
        .await
        .unwrap();

    for member in &consumer_group_info.members {
        assert_eq!(member.partitions.len(), 1);
    }
}

async fn execute_using_entity_id_key(
    system_client: &IggyClient,
    client1: &IggyClient,
    client2: &IggyClient,
    client3: &IggyClient,
) {
    // 1. Send messages to the calculated partition ID on the server side by using entity ID as a key
    for entity_id in 1..=MESSAGES_COUNT {
        let message = Message::from_str(&get_message_payload(entity_id)).unwrap();
        let messages = vec![message];
        let send_messages = SendMessages {
            stream_id: Identifier::numeric(STREAM_ID).unwrap(),
            topic_id: Identifier::numeric(TOPIC_ID).unwrap(),
            key: Key::entity_id_u32(entity_id),
            messages_count: 1,
            messages,
        };
        system_client.send_messages(&send_messages).await.unwrap();
    }

    // 2. Poll the messages for each client per assigned partition in the consumer group
    let mut total_read_messages_count = 0;
    total_read_messages_count += poll_messages(&client1).await;
    total_read_messages_count += poll_messages(&client2).await;
    total_read_messages_count += poll_messages(&client3).await;

    assert_eq!(total_read_messages_count, MESSAGES_COUNT);
}

async fn poll_messages(client: &IggyClient) -> u32 {
    let poll_messages = PollMessages {
        consumer_type: ConsumerType::ConsumerGroup,
        consumer_id: CONSUMER_GROUP_ID,
        stream_id: Identifier::numeric(STREAM_ID).unwrap(),
        topic_id: Identifier::numeric(TOPIC_ID).unwrap(),
        partition_id: 0,
        kind: Next,
        value: 0,
        count: 1,
        auto_commit: true,
        format: Format::None,
    };

    let mut total_read_messages_count = 0;
    for _ in 1..=PARTITIONS_COUNT * MESSAGES_COUNT {
        let messages = client.poll_messages(&poll_messages).await.unwrap();
        total_read_messages_count += messages.len() as u32;
    }

    total_read_messages_count
}

fn get_message_payload(entity_id: u32) -> String {
    format!("message-{}", entity_id)
}

async fn execute_using_none_key(
    system_client: &IggyClient,
    client1: &IggyClient,
    client2: &IggyClient,
    client3: &IggyClient,
) {
    // 1. Send messages to the calculated partition ID on the server side (round-robin) by using none key
    for entity_id in 1..=MESSAGES_COUNT * PARTITIONS_COUNT {
        let mut partition_id = entity_id % PARTITIONS_COUNT;
        if partition_id == 0 {
            partition_id = PARTITIONS_COUNT;
        }

        let message =
            Message::from_str(&get_extended_message_payload(partition_id, entity_id)).unwrap();
        let messages = vec![message];
        let send_messages = SendMessages {
            stream_id: Identifier::numeric(STREAM_ID).unwrap(),
            topic_id: Identifier::numeric(TOPIC_ID).unwrap(),
            key: Key::none(),
            messages_count: 1,
            messages,
        };
        system_client.send_messages(&send_messages).await.unwrap();
    }

    let consumer_group_info = system_client
        .get_consumer_group(&GetConsumerGroup {
            stream_id: STREAM_ID,
            topic_id: TOPIC_ID,
            consumer_group_id: CONSUMER_GROUP_ID,
        })
        .await
        .unwrap();

    for member in &consumer_group_info.members {
        assert_eq!(member.partitions.len(), 1);
    }

    // 2. Poll the messages for each client per assigned partition in the consumer group
    validate_message_polling(client1, &consumer_group_info).await;
    validate_message_polling(client2, &consumer_group_info).await;
    validate_message_polling(client3, &consumer_group_info).await;
}

async fn validate_message_polling(client: &IggyClient, consumer_group: &ConsumerGroupDetails) {
    let client_info = client.get_me(&GetMe {}).await.unwrap();
    let consumer_group_member = consumer_group
        .members
        .iter()
        .find(|m| m.id == client_info.id)
        .unwrap();
    let partition_id = consumer_group_member.partitions[0];
    let mut start_entity_id = partition_id % PARTITIONS_COUNT;
    if start_entity_id == 0 {
        start_entity_id = PARTITIONS_COUNT;
    }

    let poll_messages = PollMessages {
        consumer_type: ConsumerType::ConsumerGroup,
        consumer_id: CONSUMER_GROUP_ID,
        stream_id: Identifier::numeric(STREAM_ID).unwrap(),
        topic_id: Identifier::numeric(TOPIC_ID).unwrap(),
        partition_id: 0,
        kind: Next,
        value: 0,
        count: 1,
        auto_commit: true,
        format: Format::None,
    };

    for i in 1..=MESSAGES_COUNT {
        let messages = client.poll_messages(&poll_messages).await.unwrap();
        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        let offset = (i - 1) as u64;
        assert_eq!(message.offset, offset);
        let entity_id = start_entity_id + ((i - 1) * PARTITIONS_COUNT);
        let payload = from_utf8(&message.payload).unwrap();
        assert_eq!(
            payload,
            &get_extended_message_payload(partition_id, entity_id)
        );
    }

    let messages = client.poll_messages(&poll_messages).await.unwrap();
    assert!(messages.is_empty())
}

fn get_extended_message_payload(partition_id: u32, entity_id: u32) -> String {
    format!("message-{}-{}", partition_id, entity_id)
}