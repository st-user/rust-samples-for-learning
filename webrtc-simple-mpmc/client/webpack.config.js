const path = require('path');
const { VueLoaderPlugin } = require('vue-loader');
const webpack = require('webpack');

const node_env = process.env.NODE_ENV;
const isDevelopment = (!node_env || node_env === 'development');
const mode = isDevelopment ? 'development' : 'production';
console.log(`Environment: ${node_env}, mode: ${mode}`);

const configs = {
  mode,
  entry: './src/index.ts',
  module: {
    rules: [
      {
        test: /\.tsx?$/,
        loader: 'ts-loader',
        options: {
          appendTsSuffixTo: [/\.vue$/],
        },
        exclude: /node_modules/
      },
      {
        test: /\.vue$/,
        use: ['vue-loader'],
      },
      {
        test: /\.css$/,
        use: ['vue-style-loader', 'css-loader']
      }
    ],
  },
  resolve: {
    extensions: ['.tsx', '.ts', '.js', '.vue']
  },
  plugins: [
    new webpack.DefinePlugin({
      "__VUE_OPTIONS_API__": true,
      "__VUE_PROD_DEVTOOLS__": false
    }),
    new VueLoaderPlugin()
  ],
  output: {
    filename: 'main.js',
    path: path.resolve(__dirname, 'dist'),
  },
};

if (isDevelopment) {
  configs.devtool = 'inline-source-map';
  configs.devServer = {
    static: ['./dist', './public'],
    port: 9000,
    proxy: {
      '/app': 'http://localhost:9001',
      /*
      */
      '/ws-app': {
        target: 'http://localhost:9001',
        ws: true,
      }
    }
  };
}

module.exports = configs;