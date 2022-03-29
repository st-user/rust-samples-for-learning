<template>
	<h1>Simple multiple producers and multiple consumers</h1>
	<div>
		<button type="button" @click="start" :disabled="startButtonDisabled">start</button>
	</div>
	<div v-if="isStarted">
		<h2 class="publisher">My Video</h2>
		<video :src-object.prop.camel="srcObject" autoplay controls></video>
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
import { defineComponent } from 'vue';

enum AppState {
	Init,
	Started
}

interface DataType {
	srcObject: MediaStream | null,
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
	Start = 'Start',
	Prepare = 'Prepare'
}

interface SubscriberMessage {
	msg_type: SubscriberMessageType,
	message: string
}

class ConnectionHandler {

	private socket: WebSocket | undefined;
	private pc: RTCPeerConnection | undefined;
	
	async init(data: DataType): Promise<void> {

		const pc = await this.initRTCPeerConnection(data);

		const isHttps = location.protocol.startsWith('https:');
		const scheme = isHttps ? 'wss:' : 'ws:';
		this.socket = new WebSocket(`${scheme}//${location.host}/ws-app/subscribe`);

		this.socket.addEventListener('open', () => {

			this.sendMessage(JSON.stringify({
				msg_type: SubscriberMessageType.Prepare,
				message: ''
			} as SubscriberMessage));
		});
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
				await this.gatherIceCandidate(pc);
				const answer = pc.localDescription;

				if (!answer) {
					console.error('Answer is null');
					break;
				}

				this.sendMessage(JSON.stringify({
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

	private async initRTCPeerConnection(data: DataType): Promise<RTCPeerConnection> {

		const pc = this.newRTCPeerConnection();

		pc.ontrack = (event: RTCTrackEvent) => {
			const videoId = event.streams[0].id;

			console.debug('on_track', event.track);

			if (videoId.startsWith('sfu-stream-') && event.track.kind === 'video') {

				data.videos.push({
					id: videoId,
					srcObject: event.streams[0]
				} as VideoWindow);

				event.track.onmute = () => {
					console.debug(`mute ${videoId}`);
					for (let i = 0; data.videos.length; i++) {
						const video = data.videos[i];
						if (!video) {
							continue;
						}
						if (videoId === video.id) {
							console.debug(`Remove the video whose index is ${i}`);
							data.videos.splice(i, 1);
							break;
						}
					}
				};
			}
		};
		const stream = await navigator.mediaDevices.getUserMedia({
			video: true,
			audio: true
		});

		data.srcObject = stream;
		stream.getTracks()
			.forEach(track => pc.addTrack(track, stream));

		return pc;
	}

	// References
	// https://github.com/aiortc/aiortc/blob/main/examples/webcam/client.js
	// 

	private async gatherIceCandidate(pc: RTCPeerConnection): Promise<void> {
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

	private newRTCPeerConnection(): RTCPeerConnection {
		return new RTCPeerConnection({
			iceServers: [
				{
					urls: 'stun:stun.l.google.com:19302'
				}
			]
		});
	}


	private sendMessage(text: string): void {
		if (!this.socket) {
			console.error('Socket is null');
			return;
		}
		this.socket.send(text);
	}
}

const App = defineComponent({
	data(): DataType {
		return {
			srcObject: null,
			videos: [],
			state: AppState.Init
		};
	},
	methods: {
		start(): void {
			this.state = AppState.Started;
			const handler = new ConnectionHandler();
			handler.init(this);
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
	color: rgb(21, 9, 77);
}
.subscriber {
	color: rgb(90, 6, 17);
}
</style>