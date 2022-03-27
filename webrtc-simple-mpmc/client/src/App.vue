<template>
	<h1>Simple multiple producers and multiple consumers</h1>
	<div>
		<button type="button" @click="start" :disabled="startButtonDisabled">start</button>
	</div>
	<div v-if="isStarted">
		<h2 class="publisher">My Video</h2>
		<video :src-object.prop.camel="srcObject" autoplay controls :muted="muted"></video>
	</div>
	<div v-if="isStarted">
		<h2 class="subscriber">Watching</h2>
		<template v-for="video in videos" :key="video.id">
			<h3>{{ video.id }}</h3>
			<video :src-object.prop.camel="video.srcObject" autoplay controls></video>
		</template>
	</div>
</template>

<script lang="ts">
import { defineComponent, reactive } from 'vue';

enum AppState {
	Init,
	Started
}

interface DataType {
	message: string,
	srcObject: MediaStream | null,
	muted: boolean,
	videos: Array<VideoWindow>,
	state: AppState
}

interface VideoWindow {
	id: string,
	srcObject: MediaStream | null
}

enum SubscriberMessageType {
	Offer = 'Offer',
	Answer = 'Answer',
	Start = 'Start'
}

interface SubscriberMessage {
	msg_type: SubscriberMessageType,
	message: string
}


const videoHandle = reactive({
	videos: new Array<VideoWindow>()
});

// References
// https://github.com/aiortc/aiortc/blob/main/examples/webcam/client.js
// 

async function gatherIceCandidate(pc: RTCPeerConnection): Promise<void> {
	return new Promise(resolve => {

		if (pc.iceGatheringState === 'complete') {
			resolve();
		} else {
			const checkState = () => {
				if (pc.iceGatheringState === 'complete') {
					pc.removeEventListener('icegatheringstatechange', checkState);
					resolve();
				}
			};
			pc.addEventListener('icegatheringstatechange', checkState);
		}

	});
}

function newRTCPeerConnection(): RTCPeerConnection {
	return new RTCPeerConnection({
		iceServers: [
			{
				urls: 'stun:stun.l.google.com:19302'
			}
		]
	});
}

class SubscriptionHandler {

	private socket: WebSocket;
	private pc: RTCPeerConnection | undefined;

	constructor() {

		const isHttps = location.protocol.startsWith('https:');
		const scheme = isHttps ? 'wss:' : 'ws:';
		const pc = this.initRTCPeerConnection();
		this.socket = new WebSocket(`${scheme}//${location.host}/ws-app/subscribe`);
		this.socket.addEventListener('message', async (event: MessageEvent) => {

			const message = JSON.parse(event.data) as SubscriberMessage;

			switch (message.msg_type) {
			case SubscriberMessageType.Offer: {

				if (!message.message) {
					console.error('Invalid message format', message);
					break;
				}

				const offer = JSON.parse(message.message);
				console.debug(offer);
				await pc.setRemoteDescription(offer);
				await pc.setLocalDescription(await pc.createAnswer());
				await gatherIceCandidate(pc);
				const answer = pc.localDescription;

				if (!answer) {
					console.error('Answer is null');
					break;
				}

				this.socket.send(JSON.stringify({
					msg_type: SubscriberMessageType.Answer,
					message: JSON.stringify(answer)
				}));
				break;
			}
			default:
				break;
			}
		});
	}

	private initRTCPeerConnection(): RTCPeerConnection {

		const pc = newRTCPeerConnection();

		pc.ontrack = (event: RTCTrackEvent) => {
			const videoId = event.streams[0].id;

			console.debug('on_track', event.track);

			if (event.track.kind === 'video') {

				videoHandle.videos.push({
					id: videoId,
					srcObject: event.streams[0]
				} as VideoWindow);

				event.track.onmute = () => {
					console.debug(`mute ${videoId}`);
					for (let i = 0; videoHandle.videos.length; i++) {
						const video = videoHandle.videos[i];
						if (!video) {
							continue;
						}
						if (videoId === video.id) {
							console.debug(`Remove the video whose index is ${i}`);
							videoHandle.videos.splice(i, 1);
							break;
						}
					}
				};
			}
		};
		return pc;
	}

	sendMessage(text: string): void {
		this.socket.send(text);
	}
}

async function doPublish(data: DataType): Promise<void> {

	const pc = newRTCPeerConnection();

	const stream = await navigator.mediaDevices.getUserMedia({
		video: true,
		audio: true
	});

	data.srcObject = stream;
	stream.getTracks()
		.forEach(track => pc.addTrack(track, stream));

	await pc.setLocalDescription(await pc.createOffer());
	await gatherIceCandidate(pc);
	const offer = pc.localDescription as RTCSessionDescription;
	if (!offer) {
		console.error('Offer is null');
		return;
	}
	const answer = await fetch('/app/offer', {
		body: JSON.stringify({
			sdp: offer.sdp,
			type: offer.type,
		}),
		headers: {
			'Content-Type': 'application/json'
		},
		method: 'POST'
	}).then(res => res.json());

	await pc.setRemoteDescription(answer);
}

const App = defineComponent({
	data(): DataType {
		return {
			message: '',
			srcObject: null,
			muted: false,
			videos: videoHandle.videos,
			state: AppState.Init
		};
	},
	methods: {
		start(): void {
			this.state = AppState.Started;
			const subscriptionHandler = new SubscriptionHandler();
			doPublish(this).then(() => {
				subscriptionHandler.sendMessage(JSON.stringify({
					msg_type: SubscriberMessageType.Start,
					message: ''
				} as SubscriberMessage));
			});
		}
	},
	computed: {
		startButtonDisabled(): boolean {
			const data = this as DataType;
			return data.state != AppState.Init;
		},
		isStarted(): boolean {
			const data = this as DataType;
			return data.state === AppState.Started;
		}
	}
});
export default App;
</script>

<style scoped>
.publisher {
	color:rgb(21, 9, 77);
}
.subscriber {
	color:rgb(90, 6, 17);
}
</style>